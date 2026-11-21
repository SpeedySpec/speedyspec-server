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
    /// The extension's native areas: their functions and classes are registered at module startup, and their
    /// request hooks run on every request.
    pub areas: Vec<std::rc::Rc<dyn crate::NativeArea>>,
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
    /// The preload, kept to run again at the start of every later request.
    preload: Vec<(String, String)>,
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
            .send_headers_function(crate::web::send_headers)
            .read_post_function(crate::web::read_post)
            .read_cookies_function(crate::web::read_cookies)
            .register_server_variables_function(crate::web::register_server_variables)
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
            (*sapi).ini_entries = EXTRA_INI.get().copied().unwrap_or(INI_ENTRIES).as_ptr();
            crate::area::install(options.areas);
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
            preload: options
                .preload
                .iter()
                .map(|(name, source)| ((*name).to_owned(), (*source).to_owned()))
                .collect(),
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
        // Rust code reads a PHP string or array passed through it unconverted only when it looks at it.
        speedyspec_hooks::set_opaque_reader(speedyspec_hooks::OpaqueReader {
            readable: convert::readable_opaque,
            read: convert::read_opaque,
        });
        php.request_setup()?;
        Ok(php)
    }

    /// What every request gets before any other PHP code: `SpeedySpec\Hooks\HookException`, the `STDIN`,
    /// `STDOUT` and `STDERR` constants, the preload, then each area's `request_start`.
    fn request_setup(&self) -> Result<(), PhpError> {
        execute(
            HOOK_EXCEPTION_LABEL,
            HOOK_EXCEPTION_PHP.as_bytes(),
            _zend_compile_position_ZEND_COMPILE_POSITION_AFTER_OPEN_TAG,
        )?;
        hook_class::find_exception_class()?;
        execute(
            STD_HANDLES_LABEL,
            STD_HANDLES_PHP.as_bytes(),
            _zend_compile_position_ZEND_COMPILE_POSITION_AFTER_OPEN_TAG,
        )?;
        for (name, source) in &self.preload {
            execute(
                name,
                source.as_bytes(),
                _zend_compile_position_ZEND_COMPILE_POSITION_AT_OPEN_TAG,
            )?;
        }
        crate::area::request_start();
        Ok(())
    }

    /// Ends the current request and starts a fresh one, as a php-fpm worker does between requests: every PHP value
    /// of the old request is freed (each area's `request_end` runs), and the new one runs the request setup again.
    pub fn restart_request(&self) -> Result<(), PhpError> {
        self.end_request();
        self.start_request()?;
        self.request_setup()
    }

    /// Ends the open request, if one is (after [`Php::serve`] none is).
    fn end_request(&self) {
        if !crate::boundary::alive() {
            return;
        }
        settle_stray();
        // SAFETY: ends the request on the booting thread, as `Drop` does; areas' request_end runs inside it.
        unsafe { php_request_shutdown(ptr::null_mut()) };
        ENGINE.with(|e| {
            e.alive.set(false);
            let queued = std::mem::take(&mut *e.releases.borrow_mut());
            drop(queued); // ManuallyDrop: the request freed them.
            let current = e.take_current();
            let saved = std::mem::take(&mut *e.contexts.borrow_mut());
            drop(current);
            drop(saved);
        });
    }

    /// Starts a request on the booting thread, after the previous one ended.
    fn start_request(&self) -> Result<(), PhpError> {
        // SAFETY: the next request on the same thread, after the previous one ended.
        unsafe {
            if php_request_startup() != ZEND_RESULT_CODE_SUCCESS {
                return Err(PhpError::Boot("php_request_startup failed".to_owned()));
            }
            (*ext_php_rs_compiler_globals()).skip_shebang = true;
        }
        ENGINE.with(|e| e.alive.set(true));
        Ok(())
    }

    /// Serves one web request: the open request (if any) ends, `request` becomes the request PHP sees (its
    /// parameters as `$_SERVER`, its body as `php://input`, PHP's headers and output sent to it), the request setup
    /// runs (each area's `request_start`), `script` runs, and the request ends (each area's `request_end`). No
    /// request is open afterwards. An exception or fatal error before any output makes the response a 500.
    pub fn serve(&self, request: &mut dyn crate::web::WebRequest, script: &Path) -> Served {
        self.end_request();
        let param = |name: &str| {
            request
                .params()
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        };
        let c = |value: Option<String>| value.and_then(|v| CString::new(v).ok());
        let method = c(param("REQUEST_METHOD").or(Some("GET".into())));
        let query = c(param("QUERY_STRING"));
        let uri = c(param("REQUEST_URI"));
        let content_type = c(param("CONTENT_TYPE"));
        let content_length = param("CONTENT_LENGTH")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(-1);
        let script_c = c(script.to_str().map(str::to_owned));
        let ptr_of = |v: &Option<CString>| {
            v.as_ref()
                .map_or(ptr::null_mut(), |s| s.as_ptr().cast_mut())
        };
        crate::web::begin(request);
        // SAFETY: SG() on the booting thread between requests; the strings live until the request has ended.
        unsafe {
            let sg = &raw mut ext_php_rs::ffi::sapi_globals;
            (*sg).server_context = (&raw mut SERVER_CONTEXT).cast();
            (*sg).request_info.request_method = ptr_of(&method);
            (*sg).request_info.query_string = ptr_of(&query);
            (*sg).request_info.request_uri = ptr_of(&uri);
            (*sg).request_info.content_type = ptr_of(&content_type);
            (*sg).request_info.content_length = content_length;
            (*sg).request_info.path_translated = ptr_of(&script_c);
            (*sg).request_info.proto_num = 1001;
            // PHP never resets the status between requests; a SAPI does, as php-fpm does.
            (*sg).sapi_headers.http_response_code = 200;
        }
        let started = self.start_request();
        let mut served = Served {
            result: Ok(()),
            peak_memory: 0,
            poisoned: false,
            headers_sent: false,
        };
        match started {
            Err(err) => served.result = Err(err),
            Ok(()) => {
                served.result = self
                    .request_setup()
                    .and_then(|()| self.require(script).map(|_| ()))
                    .or_else(|err| match err {
                        PhpError::Exit(_) => Ok(()),
                        other => Err(other),
                    });
                if served.result.is_err() {
                    // SAFETY: SG() during the request; PHP sends whatever code is set when the headers go out.
                    unsafe {
                        let sg = &raw mut ext_php_rs::ffi::sapi_globals;
                        if (*sg).headers_sent == 0 {
                            (*sg).sapi_headers.http_response_code = 500;
                        }
                    }
                }
                // SAFETY: the engine is running a request on this thread.
                served.peak_memory = unsafe { zend_memory_peak_usage(true) } as u64;
                served.poisoned = self.poisoned();
                self.end_request();
            }
        }
        served.headers_sent = crate::web::STATE.with(|s| s.borrow().headers_sent);
        crate::web::end();
        // SAFETY: SG() between requests: the request's strings are about to be dropped.
        unsafe {
            let sg = &raw mut ext_php_rs::ffi::sapi_globals;
            (*sg).server_context = ptr::null_mut();
            (*sg).request_info.request_method = ptr::null();
            (*sg).request_info.query_string = ptr::null_mut();
            (*sg).request_info.request_uri = ptr::null_mut();
            (*sg).request_info.content_type = ptr::null();
            (*sg).request_info.content_length = 0;
            (*sg).request_info.path_translated = ptr::null_mut();
            // PHP read the cookies from `read_cookies` (a string freed with the request); a later request must not.
            (*sg).request_info.cookie_data = ptr::null_mut();
        }
        drop((method, query, uri, content_type, script_c));
        served
    }

    /// Runs PHP statements (no `<?php` tag) and returns what a top-level
    /// `return` gives back, or null. `label` names the code in PHP's error
    /// messages, where a file name would otherwise go.
    pub fn eval(&self, label: &str, code: &str) -> Result<Value, PhpError> {
        execute(
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
    /// `$_SERVER['SCRIPT_NAME']`, `SCRIPT_FILENAME` and `PHP_SELF` are the path.
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
        // As the PHP CLI sets them: a phar's stub runs only when it finds itself here.
        let quoted = php_quote(script);
        code.push_str(&format!(
            " $_SERVER['SCRIPT_NAME'] = {quoted}; $_SERVER['SCRIPT_FILENAME'] = {quoted}; \
             $_SERVER['PHP_SELF'] = {quoted};"
        ));
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
        call(callable, args)
    }
}

/// Calls a PHP callable from anywhere on the thread that booted PHP while a request is open, as [`Php::call`] does
/// (an adapter of a port holds no `Php` handle). Fails without touching PHP when no request is open.
pub fn call(callable: &Value, args: &[Value]) -> Result<Value, PhpError> {
    {
        if !crate::boundary::alive() {
            return Err(PhpError::Boot("no PHP request is open".to_owned()));
        }
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
}

/// A PHP call that failed: the error, and the thrown object when PHP threw one (a `Value::Opaque`), so a native
/// function can rethrow the original exception.
#[derive(Debug)]
pub struct CallFailure {
    pub error: PhpError,
    pub exception: Option<Value>,
}

/// As [`call`], keeping the exception object a failed call threw.
pub fn call_catching(callable: &Value, args: &[Value]) -> Result<Value, CallFailure> {
    let plain = |error: PhpError| CallFailure {
        error,
        exception: None,
    };
    if !crate::boundary::alive() {
        return Err(plain(PhpError::Boot("no PHP request is open".to_owned())));
    }
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
    settle_stray();
    outcome.map_err(failure_of)
}

/// A failed call as [`CallFailure`]: a thrown exception keeps its object.
pub(crate) fn failure_of(err: CallError) -> CallFailure {
    match err {
        CallError::Thrown(crate::boundary::Thrown::Exception {
            object,
            class,
            message,
            file,
            line,
        }) => CallFailure {
            error: PhpError::Exception {
                class,
                message,
                file,
                line,
            },
            exception: Some(Value::Opaque(std::rc::Rc::new(object))),
        },
        other => CallFailure {
            error: other.into_php_error(),
            exception: None,
        },
    }
}

/// The SAPI's INI entries with [`set_ini`]'s settings after them, once set.
static EXTRA_INI: std::sync::OnceLock<&'static std::ffi::CStr> = std::sync::OnceLock::new();

/// INI settings for every request of this process, as php.ini would give them (so `PHP_INI_PERDIR` and
/// `PHP_INI_SYSTEM` settings such as `upload_max_filesize` apply, which `ini_set()` cannot change). Call before
/// [`Php::boot`], once. A key or value with a quote, a newline or a NUL is refused.
pub fn set_ini(settings: &[(String, String)]) -> Result<(), String> {
    let mut entries = INI_ENTRIES.to_string_lossy().into_owned();
    for (key, value) in settings {
        let bad = |s: &str| s.contains(['"', '\n', '\r', '\0']) || s.is_empty();
        if bad(key) || key.contains('=') || value.contains(['"', '\n', '\r', '\0']) {
            return Err(format!(
                "the INI setting {key:?} = {value:?} cannot be passed to PHP"
            ));
        }
        entries.push_str(&format!("{key}=\"{value}\"\n"));
    }
    let entries = CString::new(entries).map_err(|err| err.to_string())?;
    EXTRA_INI
        .set(Box::leak(entries.into_boxed_c_str()))
        .map_err(|_| "the INI settings are already set".to_string())
}

/// What serving one web request came to.
#[derive(Debug)]
pub struct Served {
    /// The script's outcome; `exit()` is a normal end.
    pub result: Result<(), PhpError>,
    /// The engine's peak memory during the request, in bytes.
    pub peak_memory: u64,
    /// Whether a fatal error left the engine degraded (the worker should be replaced).
    pub poisoned: bool,
    pub headers_sent: bool,
}

/// `SG(server_context)` while a web request runs: PHP only reads the body and cookies when it is set.
static mut SERVER_CONTEXT: [u64; 4] = [0; 4];

unsafe extern "C" {
    fn zend_memory_peak_usage(real_usage: bool) -> usize;
}

/// `$GLOBALS[$name]`, through references; `None` when it is not set or no request is open. Runs no PHP code.
pub fn global(name: &str) -> Option<Value> {
    if !crate::boundary::alive() {
        return None;
    }
    // SAFETY: a request is open, so the symbol table is live; `to_value` neither runs PHP code nor allocates PHP
    // memory.
    unsafe {
        let table = &raw mut (*ext_php_rs_executor_globals()).symbol_table;
        let slot = ext_php_rs::ffi::zend_hash_str_find(table, name.as_ptr().cast(), name.len());
        if slot.is_null() {
            return None;
        }
        let value = convert::deref(slot);
        if u32::from(convert::type_of(value)) == ext_php_rs::ffi::IS_UNDEF {
            return None;
        }
        convert::to_value(value).ok()
    }
}

/// `$GLOBALS[$name] = $value`: a variable referencing the global sees the new value too.
pub fn set_global(name: &str, value: &Value) -> Result<(), PhpError> {
    if !crate::boundary::alive() {
        return Err(PhpError::Boot("no PHP request is open".to_owned()));
    }
    let mut fresh = ManuallyDrop::new(Zval::new());
    let fresh_ptr: *mut zval = &raw mut *fresh;
    let written = guard(|| {
        // SAFETY: `fresh` is zval storage owned by this frame.
        unsafe { convert::write_value(fresh_ptr, value) }
    });
    match written {
        Ok(Ok(())) => {}
        Ok(Err(why)) => {
            release_later(fresh);
            return Err(PhpError::Conversion(why.to_string()));
        }
        Err(interrupted) => return Err(fatal_error(interrupted)),
    }
    let name_ptr = name.as_ptr();
    let name_len = name.len();
    let replaced = guard(|| {
        // SAFETY: a request is open; the slot found is live; the new value's ownership moves into the table or
        // the slot, and the old value is handed back to be released.
        unsafe {
            let table = &raw mut (*ext_php_rs_executor_globals()).symbol_table;
            let slot = ext_php_rs::ffi::zend_hash_str_find(table, name_ptr.cast(), name_len);
            if slot.is_null() {
                ext_php_rs::ffi::zend_hash_str_update(table, name_ptr.cast(), name_len, fresh_ptr);
                return None;
            }
            let mut target = slot;
            if u32::from(convert::type_of(target)) == ext_php_rs::ffi::IS_INDIRECT {
                target = (*target).value.zv;
            }
            if u32::from(convert::type_of(target)) == ext_php_rs::ffi::IS_REFERENCE {
                target = &raw mut (*(*target).value.ref_).val;
            }
            let old: Zval = ptr::read(target);
            ptr::copy_nonoverlapping(fresh_ptr, target, 1);
            Some(old)
        }
    });
    match replaced {
        Ok(Some(old)) => {
            release_later(ManuallyDrop::new(old));
            settle_stray();
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(interrupted) => Err(fatal_error(interrupted)),
    }
}

/// The global `name`'s value slot (indirect slots and references followed), if it is set.
///
/// # Safety
///
/// A request is open, on the engine's thread.
pub(crate) unsafe fn global_slot(name: &str) -> Option<*mut zval> {
    // SAFETY: the symbol table is live while a request is open; indirect and reference slots point to live zvals.
    unsafe {
        let table = &raw mut (*ext_php_rs_executor_globals()).symbol_table;
        let mut slot = ext_php_rs::ffi::zend_hash_str_find(table, name.as_ptr().cast(), name.len());
        if slot.is_null() {
            return None;
        }
        if u32::from(convert::type_of(slot)) == ext_php_rs::ffi::IS_INDIRECT {
            slot = (*slot).value.zv;
        }
        if u32::from(convert::type_of(slot)) == ext_php_rs::ffi::IS_REFERENCE {
            slot = &raw mut (*(*slot).value.ref_).val;
        }
        Some(slot)
    }
}

/// `SEPARATE_ARRAY`: makes the array in `zv` one only `zv` holds, so changing it changes no copy.
///
/// # Safety
///
/// `zv` holds an array; inside a guard (duplicating allocates).
unsafe fn separate_array(zv: *mut zval) {
    // SAFETY: forwarded from the caller; an array's `arr` is its live table.
    unsafe {
        let ht = (*zv).value.arr;
        let counted = u32::from((*zv).u1.v.type_flags) & ext_php_rs::ffi::IS_TYPE_REFCOUNTED != 0;
        if counted && (*ht).gc.refcount == 1 {
            return;
        }
        let dup = ext_php_rs::ffi::zend_array_dup(ht);
        if counted {
            // Others still hold it: dropping our share frees nothing.
            (*ht).gc.refcount -= 1;
        }
        (*zv).value.arr = dup;
        (*zv).u1.type_info = ext_php_rs::ffi::IS_ARRAY_EX;
    }
}

/// `$GLOBALS[$name][] = $value`, in place, as WordPress pushes `$wp_current_filter`: one entry into PHP. False
/// (nothing done) when the global is not an array.
pub fn global_push(name: &str, value: &Value) -> Result<bool, PhpError> {
    if !crate::boundary::alive() {
        return Err(PhpError::Boot("no PHP request is open".to_owned()));
    }
    let mut fresh = ManuallyDrop::new(Zval::new());
    let fresh_ptr: *mut zval = &raw mut *fresh;
    let pushed = guard(|| -> Result<bool, convert::Unconvertible> {
        // SAFETY: a request is open; `fresh` is zval storage owned by the enclosing frame, whose ownership moves into
        // the array.
        unsafe {
            let Some(slot) = global_slot(name) else {
                return Ok(false);
            };
            if u32::from(convert::type_of(slot)) != ext_php_rs::ffi::IS_ARRAY {
                return Ok(false);
            }
            convert::write_value(fresh_ptr, value)?;
            separate_array(slot);
            if ext_php_rs::ffi::zend_hash_next_index_insert((*slot).value.arr, fresh_ptr).is_null()
            {
                // No next index (PHP warns "Cannot add element"): the value stays ours to release.
                return Ok(false);
            }
            (*fresh_ptr).u1.type_info = ext_php_rs::ffi::IS_UNDEF;
            Ok(true)
        }
    });
    release_later(fresh);
    match pushed {
        Ok(Ok(done)) => Ok(done),
        Ok(Err(why)) => Err(PhpError::Conversion(why.to_string())),
        Err(interrupted) => Err(fatal_error(interrupted)),
    }
}

/// `array_pop($GLOBALS[$name])`, in place, as WordPress pops `$wp_current_filter`: one entry into PHP. False
/// (nothing done) when the global is not an array.
pub fn global_pop(name: &str) -> Result<bool, PhpError> {
    if !crate::boundary::alive() {
        return Err(PhpError::Boot("no PHP request is open".to_owned()));
    }
    let popped = guard(|| {
        // SAFETY: a request is open; the array's slots are read as `convert::array_values` does, and the last one is
        // deleted through the hash API (which releases it, inside this guard).
        unsafe {
            let Some(slot) = global_slot(name) else {
                return false;
            };
            if u32::from(convert::type_of(slot)) != ext_php_rs::ffi::IS_ARRAY {
                return false;
            }
            if (*(*slot).value.arr).nNumOfElements == 0 {
                return true;
            }
            separate_array(slot);
            convert::pop_last((*slot).value.arr);
            true
        }
    });
    match popped {
        Ok(done) => {
            settle_stray();
            Ok(done)
        }
        Err(interrupted) => Err(fatal_error(interrupted)),
    }
}

/// `spl_object_id($value)` for a PHP object held as a `Value::Opaque`.
pub fn object_id(value: &Value) -> Option<u64> {
    let object = convert::handle_of(value)?.object()?;
    // SAFETY: the handle holds a reference to the object, so it is live.
    Some(u64::from(unsafe { (*object).handle }))
}

impl Php {
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
    fn execute_in(
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
        // SAFETY: ends the request on the booting thread, if one is open (after `serve` none is). Native functions
        // still work during it (shutdown functions and destructors may fire hooks); PHP catches bailouts in each
        // shutdown step itself.
        if crate::boundary::alive() {
            unsafe { php_request_shutdown(ptr::null_mut()) };
        }

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
        crate::area::uninstall();

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

/// Compiles and runs `code` in the open request; for [`Php`]'s methods and the free [`eval`].
fn execute(label: &str, code: &[u8], position: zend_compile_position) -> Result<Value, PhpError> {
    Php::execute_in(label, code, position)
}

/// Runs PHP statements (no `<?php` tag) from anywhere on the thread that booted PHP while a request is open, as
/// [`Php::eval`] does (a native area holds no `Php` handle). Fails without touching PHP when no request is open.
pub fn eval(label: &str, code: &str) -> Result<Value, PhpError> {
    if !crate::boundary::alive() {
        return Err(PhpError::Boot("no PHP request is open".to_owned()));
    }
    execute(
        label,
        code.as_bytes(),
        _zend_compile_position_ZEND_COMPILE_POSITION_AFTER_OPEN_TAG,
    )
}
