//! Boots the embedded PHP engine, runs code in it, and keeps PHP's error
//! handling on PHP's side of the boundary.
//!
//! The engine runs under a small custom SAPI built with ext-php-rs's
//! `SapiBuilder` rather than PHP's stock embed SAPI, because only
//! `php_module_startup` lets us hand PHP our own module: that module is how
//! functions and classes implemented in Rust become visible to PHP.
//!
//! The boundary rules (bailouts, exceptions, `exit()`, fibers) live in
//! [`crate::boundary`]; the SAPI's INI entries and callbacks in [`crate::sapi`].

use std::cell::Cell;
use std::ffi::CString;
use std::fmt;
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::panic::resume_unwind;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use ext_php_rs::builders::SapiBuilder;
use ext_php_rs::embed::{
    SapiModule, cleanup_sapi_allocations, ext_php_rs_sapi_shutdown, ext_php_rs_sapi_startup,
};
use ext_php_rs::ffi::{
    _efree, _zend_compile_position_ZEND_COMPILE_POSITION_AFTER_OPEN_TAG,
    _zend_compile_position_ZEND_COMPILE_POSITION_AT_OPEN_TAG, ZEND_RESULT_CODE_SUCCESS,
    destroy_op_array, ext_php_rs_compiler_globals, ext_php_rs_executor_globals,
    ext_php_rs_zend_string_init, ext_php_rs_zend_string_release, php_module_shutdown,
    php_module_startup, php_request_shutdown, php_request_startup, sapi_shutdown, sapi_startup,
    zend_compile_position, zend_compile_string, zend_destroy_static_vars, zend_execute,
    zend_get_executed_scope, zend_op_array, zend_string, zval,
};
use ext_php_rs::types::Zval;
use speedyspec_hooks::{Key, SharedHooks, Value};

use crate::boundary::{
    CallError, ENGINE, call_php, class_name, discard_exception, drain, fatal_error, find_function,
    guard, handle_queued_oom, release_later, settle_stray, table_as_zval, take_thrown,
};
use crate::convert;
use crate::hook_class;
use crate::sapi::{INI_ENTRIES, OUTPUT_TO_STDERR, flush, log_message, ub_write};

/// `SpeedySpec\Hooks\HookException`, evaluated at boot before the preload.
const HOOK_EXCEPTION_PHP: &str = r"namespace SpeedySpec\Hooks;

/**
 * Reports a hook failure that is not a callback's own exception, such as a broken backend.
 */
class HookException extends \RuntimeException
{
}
";

/// The CLI's `STDIN`, `STDOUT` and `STDERR` constants, evaluated at boot: PHP's
/// CLI SAPI defines them (`sapi/cli/php_cli.c`, `cli_register_file_handles`)
/// and scripts expect `fwrite(STDERR, ...)` to work. They are plain
/// `php://std*` streams on the process's file descriptors, so a write to
/// `STDOUT` reaches fd 1 directly and is not affected by
/// [`Php::redirect_output`], exactly as `php://stdout` is not.
const STD_HANDLES_PHP: &str = r"\define('STDIN', \fopen('php://stdin', 'rb'));
\define('STDOUT', \fopen('php://stdout', 'wb'));
\define('STDERR', \fopen('php://stderr', 'wb'));
";

/// Stands in for the file name of [`STD_HANDLES_PHP`] in PHP's messages.
const STD_HANDLES_LABEL: &str = "speedyspec://std-handles";

/// Stands in for the file name of [`HOOK_EXCEPTION_PHP`] in PHP's messages.
const HOOK_EXCEPTION_LABEL: &str = "speedyspec://hook-exception";

/// An internal function whose `zend_function` gives a dummy frame something
/// to point at; any internal function would do.
const DUMMY_FRAME_FUNCTION: &str = "strlen";

static BOOTED: AtomicBool = AtomicBool::new(false);

/// Why PHP could not boot or run something.
#[derive(Debug)]
#[non_exhaustive]
pub enum PhpError {
    /// The engine could not start (or was already started in this process).
    Boot(String),
    /// PHP code threw something that nothing caught.
    Exception {
        class: String,
        message: String,
        file: String,
        line: u32,
    },
    /// PHP hit a fatal error and abandoned the code it was running. The
    /// engine is usable afterwards but degraded; see [`Php::poisoned`].
    Fatal {
        message: String,
        file: String,
        line: u32,
    },
    /// The code called `exit()` or `die()` with this status.
    Exit(i32),
    /// A label, code string or path contained a NUL byte.
    NulByte,
    /// The script passed to `run_file` does not exist.
    ScriptNotFound(PathBuf),
    /// The path is not valid UTF-8, so it cannot be handed to PHP as `$argv`.
    NonUtf8Path(PathBuf),
    /// A value could not be converted between PHP and Rust.
    Conversion(String),
}

impl fmt::Display for PhpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boot(why) => write!(f, "PHP failed to start: {why}"),
            Self::Exception {
                class,
                message,
                file,
                line,
            } => write!(f, "{file}:{line}: uncaught {class}: {message}"),
            Self::Fatal {
                message,
                file,
                line,
            } => write!(f, "{file}:{line}: PHP fatal error: {message}"),
            Self::Exit(status) => write!(f, "PHP code called exit({status})"),
            Self::NulByte => f.write_str("PHP input contains a NUL byte"),
            Self::ScriptNotFound(path) => write!(f, "PHP script not found: {}", path.display()),
            Self::NonUtf8Path(path) => {
                write!(f, "PHP script path is not valid UTF-8: {}", path.display())
            }
            Self::Conversion(why) => write!(f, "PHP value conversion failed: {why}"),
        }
    }
}

impl std::error::Error for PhpError {}

/// Where PHP's output (`echo`, error display, ...) is written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Output {
    #[default]
    Stdout,
    Stderr,
}

/// What [`Php::boot`] needs.
pub struct BootOptions<'a> {
    /// The hook registry PHP's `SpeedySpec\Hooks\Hook` uses.
    pub hooks: SharedHooks,
    /// `(file name, PHP source)` pairs evaluated at boot, in order, before
    /// any other PHP code. The source is a file's content, `<?php` tag and
    /// all; the name stands in for the file name in PHP's messages.
    pub preload: &'a [(&'a str, &'a str)],
}

/// A booted PHP engine with one request open.
///
/// There is at most one per process, and it is neither `Send` nor `Sync`:
/// the build is non-thread-safe, so PHP must only be touched from the thread
/// that booted it. Dropping it shuts PHP down.
///
/// # PHP values that outlive the engine
///
/// Dropping `Php` first releases every PHP value Rust has already let go of,
/// then ends the request, which destroys every PHP object, including those
/// Rust still holds (PHP callbacks in the registry, `Value::Opaque`s kept by
/// Rust code). Those handles are inert from then on: dropping one releases
/// nothing (the engine freed the memory), and firing a hook whose callback is
/// a PHP callable returns a `CallbackError` without touching PHP.
pub struct Php {
    sapi: *mut SapiModule,
    _not_send: PhantomData<*mut ()>,
}

impl Php {
    /// Starts PHP, opens the request that all later calls run in, defines
    /// `SpeedySpec\Hooks\HookException` and the CLI's `STDIN`, `STDOUT` and
    /// `STDERR` constants, and evaluates `options.preload`.
    ///
    /// php.ini and the INI scan directory are never read. Like the PHP CLI,
    /// the engine skips a leading `#!` line in every file it compiles.
    pub fn boot(options: BootOptions<'_>) -> Result<Self, PhpError> {
        if BOOTED.swap(true, Ordering::SeqCst) {
            return Err(PhpError::Boot(
                "PHP is already booted in this process".to_owned(),
            ));
        }

        let sapi = SapiBuilder::new("speedyspec", "SpeedySpec")
            .ub_write_function(ub_write)
            .flush_function(flush)
            .log_message_function(log_message)
            .php_ini_ignore(1)
            .build()
            .map_err(|e| PhpError::Boot(format!("building the SAPI: {e}")))?;
        // PHP copies the struct, but cleanup_sapi_allocations needs the
        // original after shutdown, so it lives on the heap until Drop. If
        // startup fails it is leaked: a half-started engine cannot be shut
        // down safely, and the process is about to report the error and exit.
        let sapi = Box::into_raw(Box::new(sapi));

        // SAFETY: the documented SAPI start sequence, run once per process
        // (guarded by BOOTED) on this thread. `sapi_startup` clears
        // `ini_entries`, so it is set between the two startup calls, as
        // php-src's own SAPIs do. INI_ENTRIES is 'static.
        unsafe {
            ext_php_rs_sapi_startup();
            sapi_startup(sapi);
            (*sapi).ini_entries = INI_ENTRIES.as_ptr();
            if php_module_startup(sapi, hook_class::module_entry()) != ZEND_RESULT_CODE_SUCCESS {
                return Err(PhpError::Boot("php_module_startup failed".to_owned()));
            }
            if php_request_startup() != ZEND_RESULT_CODE_SUCCESS {
                return Err(PhpError::Boot("php_request_startup failed".to_owned()));
            }
            // As the CLI does; request startup has just reset it.
            (*ext_php_rs_compiler_globals()).skip_shebang = true;
        }

        // From here on, failing drops `php`, which shuts the engine down.
        let php = Self {
            sapi,
            _not_send: PhantomData,
        };

        // SAFETY: the request is open, so the function table is populated
        // and stays so until shutdown.
        let dummy = unsafe { find_function(DUMMY_FRAME_FUNCTION) };
        if dummy.is_null() {
            return Err(PhpError::Boot(format!(
                "the native function {DUMMY_FRAME_FUNCTION}() is not registered"
            )));
        }
        ENGINE.with(|e| {
            e.dummy_func.set(dummy);
            e.alive.set(true);
        });
        hook_class::install(options.hooks);
        php.execute(
            HOOK_EXCEPTION_LABEL,
            HOOK_EXCEPTION_PHP.as_bytes(),
            _zend_compile_position_ZEND_COMPILE_POSITION_AFTER_OPEN_TAG,
        )?;
        hook_class::find_exception_class()?;
        php.execute(
            STD_HANDLES_LABEL,
            STD_HANDLES_PHP.as_bytes(),
            _zend_compile_position_ZEND_COMPILE_POSITION_AFTER_OPEN_TAG,
        )?;
        for (name, source) in options.preload {
            php.execute(
                name,
                source.as_bytes(),
                _zend_compile_position_ZEND_COMPILE_POSITION_AT_OPEN_TAG,
            )?;
        }
        Ok(php)
    }

    /// Runs PHP statements (no `<?php` tag) and returns what a top-level
    /// `return` gives back, or null. `label` names the code in PHP's error
    /// messages, where a file name would otherwise go.
    pub fn eval(&self, label: &str, code: &str) -> Result<Value, PhpError> {
        self.execute(
            label,
            code.as_bytes(),
            _zend_compile_position_ZEND_COMPILE_POSITION_AFTER_OPEN_TAG,
        )
    }

    /// `require`s a PHP file and returns its return value (1 if it returns
    /// nothing). A relative path is resolved against the working directory,
    /// never against `include_path`. Exceptions, fatal errors and `exit()`
    /// come back as errors.
    pub fn require(&self, path: &Path) -> Result<Value, PhpError> {
        let label = path
            .to_str()
            .ok_or_else(|| PhpError::NonUtf8Path(path.to_owned()))?;
        if label.contains('\0') {
            return Err(PhpError::NulByte);
        }
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
        let absolute = absolute
            .to_str()
            .ok_or_else(|| PhpError::NonUtf8Path(path.to_owned()))?;
        let code = format!("return require {};", php_quote(absolute));
        self.eval(label, &code)
    }

    /// Runs a PHP file the way `php <path> <argv...>` would and returns its
    /// exit status: 0 when it ends normally, `n` when it calls `exit(n)`.
    /// `$argv` (and `$_SERVER['argv']`) is `[path, ...argv]` and `$argc` its
    /// length. An uncaught exception or fatal error is an `Err`.
    pub fn run_file(&self, path: &Path, argv: &[String]) -> Result<i32, PhpError> {
        let script = path
            .to_str()
            .ok_or_else(|| PhpError::NonUtf8Path(path.to_owned()))?;
        if !path.is_file() {
            return Err(PhpError::ScriptNotFound(path.to_owned()));
        }

        let mut code = String::from("$GLOBALS['argv'] = [");
        for arg in std::iter::once(script).chain(argv.iter().map(String::as_str)) {
            code.push_str(&php_quote(arg));
            code.push(',');
        }
        code.push_str(
            "]; $GLOBALS['argc'] = \\count($GLOBALS['argv']); \
             $_SERVER['argv'] = $GLOBALS['argv']; $_SERVER['argc'] = $GLOBALS['argc'];",
        );
        self.eval("speedyspec://argv", &code)?;

        // SAFETY: plain write to the executor globals on the engine's thread.
        unsafe { (*ext_php_rs_executor_globals()).exit_status = 0 };
        match self.require(path) {
            Ok(_) => Ok(0),
            Err(PhpError::Exit(status)) => Ok(status),
            Err(err) => Err(err),
        }
    }

    /// Calls a PHP callable (a function name, a `[class, method]` array, a
    /// closure or an invokable object) with `args` and returns its result.
    pub fn call(&self, callable: &Value, args: &[Value]) -> Result<Value, PhpError> {
        settle_stray();
        let mut target = ManuallyDrop::new(Zval::new());
        let target_ptr: *mut zval = &raw mut *target;
        let written = guard(|| {
            // SAFETY: `target` is fresh zval storage owned by this frame.
            unsafe { convert::write_value(target_ptr, callable) }
        });
        let outcome = match written {
            Ok(Ok(())) => call_php(target_ptr, args),
            Ok(Err(why)) => Err(CallError::Convert(why.to_string())),
            Err(interrupted) => Err(CallError::Interrupted(interrupted)),
        };
        release_later(target);
        // Releasing the target can run a destructor; its exception is not
        // this call's.
        settle_stray();
        outcome.map_err(CallError::into_php_error)
    }

    /// Creates a PHP object: `new $class(...$args)`. An unknown class or a
    /// constructor that throws is the exception PHP raised.
    pub fn construct(&self, class: &str, args: &[Value]) -> Result<Value, PhpError> {
        // Built in PHP rather than through the object API, so the constructor
        // runs exactly as it would for a plugin's own `new`.
        let factory = self.eval(
            "speedyspec:construct",
            "return static fn (string $class, array $arguments) => new $class(...$arguments);",
        )?;
        let arguments = Value::Array(
            args.iter()
                .enumerate()
                .map(|(index, value)| (Key::Int(index as i64), value.clone()))
                .collect(),
        );
        self.call(&factory, &[Value::Str(class.to_owned()), arguments])
    }

    /// The class of a PHP object (`Value::Opaque` from PHP); `None` for
    /// anything else.
    pub fn class_of(&self, value: &Value) -> Option<String> {
        let obj = convert::handle_of(value)?.object()?;
        // SAFETY: the handle keeps the object, and so its class, alive.
        Some(unsafe { class_name(obj) })
    }

    /// The public properties of a PHP object, in declaration then insertion
    /// order; `None` for anything that is not a PHP object, or if one of the
    /// properties cannot be converted.
    pub fn properties_of(&self, value: &Value) -> Option<Vec<(String, Value)>> {
        let obj = convert::handle_of(value)?.object()?;
        settle_stray();
        // Fetching the table can run PHP code (a lazy object initialises).
        let table = guard(|| {
            // SAFETY: the handle keeps `obj` alive; every object has handlers
            // with a get_properties entry.
            unsafe { (*(*obj).handlers).get_properties.map(|get| get(obj)) }
        });
        let table = match table {
            Ok(Some(table)) if !table.is_null() => table,
            _ => return None,
        };
        if let Some(thrown) = take_thrown() {
            drop(thrown);
            settle_stray();
            return None;
        }
        let mut props = Vec::new();
        // SAFETY: the object owns `table` and is kept alive by the handle;
        // reading it runs no PHP code.
        let read = unsafe { convert::to_value(&raw const *table_as_zval(table)) };
        let Ok(Value::Array(items)) = read else {
            return None;
        };
        for (key, item) in items {
            match key {
                // Protected and private names are mangled as "\0*\0name" and
                // "\0Class\0name".
                speedyspec_hooks::Key::Str(name) if name.starts_with('\0') => {}
                speedyspec_hooks::Key::Str(name) => props.push((name, item)),
                speedyspec_hooks::Key::Int(i) => props.push((i.to_string(), item)),
            }
        }
        Some(props)
    }

    /// True once PHP has bailed out (a fatal error, memory exhaustion, or a
    /// Rust panic carried through PHP) at least once; never cleared.
    ///
    /// The engine keeps working after a bailout, but degraded for the rest of
    /// its single request: PHP turns the cycle collector off (`gc_protect`),
    /// so reference cycles are no longer freed; a fatal error during
    /// compilation leaves the file it was compiling open and its compiler
    /// state behind; a fatal error part-way through an engine operation can
    /// leave engine structures half-updated; and destructors of every object
    /// that existed at the time of a fatal error never run. `exit()` does not
    /// poison: in PHP 8 it unwinds as an exception, not a bailout. Memory
    /// exhaustion does: PHP unwinds with the same object as `exit()`, but
    /// reports a fatal error (a bailout of its own) on the way, and it is
    /// returned as [`PhpError::Fatal`]. The caller decides whether to keep
    /// going.
    pub fn poisoned(&self) -> bool {
        ENGINE.with(|e| e.poisoned.get())
    }

    /// Names the plugin that later `Hook::add` calls are registered for, until
    /// changed. Hook errors from a failing callback name its owner.
    pub fn set_owner(&self, owner: Option<&str>) {
        hook_class::set_owner(owner);
    }

    /// Sends PHP's output to stdout (the default) or stderr.
    pub fn redirect_output(&self, to: Output) {
        flush(ptr::null_mut());
        OUTPUT_TO_STDERR.store(to == Output::Stderr, Ordering::SeqCst);
    }

    /// Compiles `code` as `label` and runs it at the current scope.
    fn execute(
        &self,
        label: &str,
        code: &[u8],
        position: zend_compile_position,
    ) -> Result<Value, PhpError> {
        let label = CString::new(label).map_err(|_| PhpError::NulByte)?;
        let label_ptr = label.as_ptr();
        settle_stray();

        // Owned here, outside every guard; the guards only see pointers.
        let mut retval = ManuallyDrop::new(Zval::new());
        let retval_ptr: *mut zval = &raw mut *retval;
        let source: Cell<*mut zend_string> = Cell::new(ptr::null_mut());
        let op_array: Cell<*mut zend_op_array> = Cell::new(ptr::null_mut());
        // SAFETY: plain reads of the compiler globals on the engine's thread.
        let saved_options = unsafe { (*ext_php_rs_compiler_globals()).compiler_options };

        let ran = guard(|| {
            // SAFETY: the sequence of zend_eval_stringl, keeping the op array
            // so its top-level `return` reaches `retval`. All pointers are
            // owned by the enclosing frame.
            unsafe {
                source.set(ext_php_rs_zend_string_init(
                    code.as_ptr().cast(),
                    code.len(),
                    false,
                ));
                let cg = ext_php_rs_compiler_globals();
                (*cg).compiler_options = 0; // ZEND_COMPILE_DEFAULT_FOR_EVAL
                let compile = zend_compile_string.expect("zend_compile_string is always set");
                let op = compile(source.get(), label_ptr, position);
                (*cg).compiler_options = saved_options;
                if op.is_null() {
                    return; // A parse error is pending in EG(exception).
                }
                op_array.set(op);
                (*op).scope = zend_get_executed_scope();
                let eg = ext_php_rs_executor_globals();
                (*eg).no_extensions = true;
                zend_execute(op, retval_ptr);
                (*eg).no_extensions = false;
                handle_queued_oom();
                zend_destroy_static_vars(op);
                destroy_op_array(op);
                _efree(op.cast());
                op_array.set(ptr::null_mut());
            }
        });

        // SAFETY: restoring what the guard set, in case it bailed out early.
        unsafe {
            (*ext_php_rs_compiler_globals()).compiler_options = saved_options;
            (*ext_php_rs_executor_globals()).no_extensions = false;
        }
        let fatal = ran.err().map(fatal_error);
        let op = op_array.get();
        if !op.is_null() {
            // A bailout during execution: free the op array as
            // zend_eval_stringl does. Refused if a re-raise is pending, in
            // which case it stays with the request until shutdown.
            let _ = guard(|| {
                // SAFETY: `op` was compiled above and not yet destroyed.
                unsafe {
                    destroy_op_array(op);
                    _efree(op.cast());
                }
            });
        }
        let src = source.get();
        if !src.is_null() {
            // SAFETY: the reference created above; freeing a string runs no
            // PHP code.
            unsafe { ext_php_rs_zend_string_release(src) };
        }

        // SAFETY: `retval` is either still null or what the code returned.
        let value = unsafe { convert::to_value(retval_ptr) };
        release_later(retval);
        drain();
        if let Some(fatal) = fatal {
            discard_exception();
            return Err(fatal);
        }
        if let Some(thrown) = take_thrown() {
            return Err(thrown.into_php_error());
        }
        value.map_err(|e| PhpError::Conversion(e.to_string()))
    }
}

impl Drop for Php {
    fn drop(&mut self) {
        // Release everything Rust has let go of while PHP code may still run,
        // and clear an exception a destructor left, which shutdown must not
        // find pending.
        settle_stray();
        // SAFETY: ends the request on the booting thread. Native functions
        // still work during it (shutdown functions and destructors may fire
        // hooks); PHP catches bailouts in each shutdown step itself.
        unsafe { php_request_shutdown(ptr::null_mut()) };

        // The request freed every PHP value. Any reference Rust still holds
        // or has queued points into freed memory and must never be touched.
        // Per-context records of fibers that were never unwound (PHP only
        // abandons a suspended fiber without unwinding it when its destructor
        // is skipped, after a fatal error) are forgotten too: `alive` is false,
        // so dropping them releases nothing.
        let parked_panic = ENGINE.with(|e| {
            e.alive.set(false);
            let queued = std::mem::take(&mut *e.releases.borrow_mut());
            drop(queued); // ManuallyDrop: nothing is released.
            let current = e.take_current();
            let saved = std::mem::take(&mut *e.contexts.borrow_mut());
            drop(current);
            drop(saved);
            e.panic.borrow_mut().take()
        });
        hook_class::uninstall();

        // SAFETY: reverse of the start sequence in `boot`, on the same thread.
        // The SAPI module is reclaimed only after PHP no longer uses it.
        unsafe {
            php_module_shutdown();
            sapi_shutdown();
            ext_php_rs_sapi_shutdown();
            cleanup_sapi_allocations(self.sapi);
            drop(Box::from_raw(self.sapi));
        }

        // A Rust panic inside a native function called during shutdown.
        if let Some(payload) = parked_panic
            && !std::thread::panicking()
        {
            resume_unwind(payload);
        }
    }
}

/// `s` as a single-quoted PHP string literal.
fn php_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\\' || c == '\'' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('\'');
    out
}
