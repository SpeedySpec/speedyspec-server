//! The PHP side of the hook registry: the final class
//! `SpeedySpec\Hooks\Hook` with static methods, backed by the [`SharedHooks`]
//! passed to `Php::boot`. There are no global functions: WordPress declares its
//! own `add_action` and friends, and must be able to load beside this.
//!
//! A PHP callable registered here is wrapped in a Rust [`Callback`] that
//! calls it through [`boundary::call_php`]; Rust callbacks in the same
//! registry run without entering PHP at all.
//!
//! The handlers are written by hand rather than with `#[php_impl]`, so that
//! every one of them goes through [`boundary::native_entry`], which is what
//! keeps bailouts and panics from crossing Rust frames.

use std::cell::{Cell, RefCell};
use std::ffi::CString;
use std::rc::Rc;

use ext_php_rs::args::Arg;
use ext_php_rs::builders::{ClassBuilder, FunctionBuilder, ModuleBuilder};
use ext_php_rs::ffi::{
    IS_ARRAY, IS_DOUBLE, IS_LONG, IS_NULL, IS_OBJECT, IS_STRING, IS_UNDEF,
    ext_php_rs_executor_globals, zend_ce_argument_count_error, zend_ce_type_error,
    zend_ce_value_error, zend_class_entry, zend_hash_index_find, zend_hash_str_find,
    zend_is_callable, zend_throw_error, zend_throw_exception, zval,
};
use ext_php_rs::flags::{ClassFlags, DataType, MethodFlags};
use ext_php_rs::prelude::*;
use ext_php_rs::types::Zval;
use ext_php_rs::zend::{ExecuteData, ModuleEntry};
use speedyspec_hooks::{
    AddOptions, Callback, CallbackError, HookError, HookErrorKind, SharedHooks, Value,
};

use crate::boundary::{self, CallError, Thrown};
use crate::convert::{self, ZvalHandle};
use crate::runtime::PhpError;

// PHP's numeric-string parser (`Zend/zend_operators.h`), a ZEND_API symbol of
// the linked libphp. ZEND_FASTCALL is a plain C call on x86_64 and aarch64.
unsafe extern "C" {
    fn _is_numeric_string_ex(
        str_: *const std::ffi::c_char,
        length: usize,
        lval: *mut i64,
        dval: *mut f64,
        allow_errors: bool,
        oflow_info: *mut std::ffi::c_int,
        trailing_data: *mut bool,
    ) -> u8;
}

const CLASS: &str = "SpeedySpec\\Hooks\\Hook";
/// The lower-cased name under which the engine files HookException.
const EXCEPTION_KEY: &str = "speedyspec\\hooks\\hookexception";

/// What the handlers need from `Php::boot`.
struct Bridge {
    hooks: SharedHooks,
    owner: Option<String>,
}

thread_local! {
    static BRIDGE: RefCell<Option<Bridge>> = const { RefCell::new(None) };
    static EXCEPTION_CLASS: Cell<*mut zend_class_entry> = const { Cell::new(std::ptr::null_mut()) };
}

pub(crate) fn install(hooks: SharedHooks) {
    BRIDGE.with(|b| *b.borrow_mut() = Some(Bridge { hooks, owner: None }));
}

/// Forgets the registry (after the engine is gone).
pub(crate) fn uninstall() {
    let bridge = BRIDGE.with(|b| b.borrow_mut().take());
    EXCEPTION_CLASS.with(|c| c.set(std::ptr::null_mut()));
    drop(bridge);
}

pub(crate) fn set_owner(owner: Option<&str>) {
    BRIDGE.with(|b| {
        if let Some(bridge) = b.borrow_mut().as_mut() {
            bridge.owner = owner.map(str::to_owned);
        }
    });
}

/// Finds `SpeedySpec\Hooks\HookException`, defined by PHP code at boot.
pub(crate) fn find_exception_class() -> Result<(), PhpError> {
    // SAFETY: the request is open, so the class table is live; it holds the
    // class entry as an IS_PTR zval.
    let ce = unsafe {
        let table = (*ext_php_rs_executor_globals()).class_table;
        let zv = zend_hash_str_find(table, EXCEPTION_KEY.as_ptr().cast(), EXCEPTION_KEY.len());
        if zv.is_null() {
            std::ptr::null_mut()
        } else {
            (*zv).value.ce
        }
    };
    if ce.is_null() {
        return Err(PhpError::Boot(
            "SpeedySpec\\Hooks\\HookException was not defined".to_owned(),
        ));
    }
    EXCEPTION_CLASS.with(|c| c.set(ce));
    Ok(())
}

// ---------------------------------------------------------------------------
// Registration

/// The module `php_module_startup` registers. It declares no global functions, so
/// code that brings its own `add_action` and friends (WordPress) loads without a clash;
/// the class comes from [`startup`].
#[php_module]
#[php(startup = startup)]
pub fn module(module: ModuleBuilder) -> ModuleBuilder {
    module
}

/// The module entry for `php_module_startup`.
pub(crate) fn module_entry() -> *mut ModuleEntry {
    get_module()
}

/// Registers the class from the module's MINIT. Returns 0 on success.
fn startup(_ty: i32, _module_number: i32) -> i32 {
    let registered = std::panic::catch_unwind(|| {
        boundary::register_fiber_observers();
        let method = MethodFlags::Public | MethodFlags::Static;
        ClassBuilder::new(CLASS)
            .flags(ClassFlags::Final)
            .method(add_signature("add", hook_add), method)
            .method(remove_signature("remove", hook_remove), method)
            .method(has_signature("has", hook_has), method)
            .method(run_signature("run", hook_run), method)
            .method(filter_signature("filter", hook_filter), method)
            .method(ran_signature("ran", hook_ran), method)
            // The engine points at the argument info for the life of the
            // process, which is how long the class lives.
            .registration(|_ce, arg_info| std::mem::forget(arg_info))
            .register()
    });
    match registered {
        Ok(Ok(())) => 0,
        _ => 1,
    }
}

fn add_signature(name: &str, handler: Handler) -> FunctionBuilder<'static> {
    FunctionBuilder::new(name, handler)
        .arg(Arg::new("hook", DataType::String))
        .arg(Arg::new("callback", DataType::Callable))
        .not_required()
        .arg(Arg::new("priority", DataType::Long).default("10"))
        .arg(Arg::new("acceptedArgs", DataType::Long).default("1"))
        .returns(DataType::True, false, false)
}

fn remove_signature(name: &str, handler: Handler) -> FunctionBuilder<'static> {
    FunctionBuilder::new(name, handler)
        .arg(Arg::new("hook", DataType::String))
        .arg(Arg::new("callback", DataType::Callable))
        .not_required()
        .arg(Arg::new("priority", DataType::Long).default("10"))
        .returns(DataType::Bool, false, false)
}

fn has_signature(name: &str, handler: Handler) -> FunctionBuilder<'static> {
    FunctionBuilder::new(name, handler)
        .arg(Arg::new("hook", DataType::String))
        .not_required()
        .arg(
            Arg::new("callback", DataType::Callable)
                .allow_null()
                .default("null"),
        )
        .returns(DataType::Bool, false, false)
}

fn run_signature(name: &str, handler: Handler) -> FunctionBuilder<'static> {
    FunctionBuilder::new(name, handler)
        .arg(Arg::new("hook", DataType::String))
        .arg(Arg::new("args", DataType::Mixed).is_variadic())
        .returns(DataType::Void, false, false)
}

fn filter_signature(name: &str, handler: Handler) -> FunctionBuilder<'static> {
    FunctionBuilder::new(name, handler)
        .arg(Arg::new("hook", DataType::String))
        .arg(Arg::new("value", DataType::Mixed))
        .arg(Arg::new("args", DataType::Mixed).is_variadic())
        .returns(DataType::Mixed, false, true)
}

fn ran_signature(name: &str, handler: Handler) -> FunctionBuilder<'static> {
    FunctionBuilder::new(name, handler)
        .arg(Arg::new("hook", DataType::String))
        .returns(DataType::Long, false, false)
}

type Handler = extern "C" fn(&mut ExecuteData, &mut Zval);

macro_rules! handler {
    ($name:ident, $body:ident) => {
        extern "C" fn $name(ex: &mut ExecuteData, retval: &mut Zval) {
            boundary::native_entry(ex, retval, |ex, retval| {
                finish(retval, $body(&Call::new(ex)))
            });
        }
    };
}

handler!(hook_add, add);
handler!(hook_remove, remove);
handler!(hook_has, has);
handler!(hook_run, run);
handler!(hook_filter, filter);
handler!(hook_ran, ran);

// ---------------------------------------------------------------------------
// The methods

/// What a method returns to PHP.
enum Ret {
    Null,
    Bool(bool),
    Long(i64),
    Value(Value),
}

/// How a method fails in PHP.
enum Fail {
    ArgumentCount(String),
    Type(String),
    Value(String),
    /// A failure reported as `SpeedySpec\Hooks\HookException`.
    Hook(String),
    /// A PHP callback's own exception, rethrown unchanged.
    Rethrow(ZvalHandle),
    /// PHP is unwinding (bailout or exit()); throw nothing.
    Silent,
}

fn add(call: &Call<'_>) -> Result<Ret, Fail> {
    call.arity(2, 4)?;
    let hook = call.hook_name()?;
    let callback = call.arg(1);
    let callable = boundary::guard(|| {
        // SAFETY: `callback` is an argument slot of the live call frame.
        unsafe { zend_is_callable(callback.cast_mut(), 0, std::ptr::null_mut()) }
    })
    .map_err(|_| Fail::Silent)?;
    if !callable {
        return Err(call.type_error(2, "callback", "a valid callback"));
    }
    let key =
        callable_key(callback).ok_or_else(|| call.type_error(2, "callback", "a valid callback"))?;
    let priority = call.priority(2)?;
    let accepted = call.int_or(3, "acceptedArgs", 1)?;
    let accepted_args = usize::try_from(accepted).map_err(|_| {
        Fail::Value(format!(
            "{}(): Argument #4 ($acceptedArgs) must be greater than or equal to 0",
            call.name
        ))
    })?;

    // SAFETY: a live argument, dereferenced; the handle takes its own reference.
    let handle = unsafe { ZvalHandle::copy_of(callback) };
    let php_callback: Rc<Callback> = Rc::new(move |args: &[Value]| run_php_callback(&handle, args));
    let (hooks, owner) = bridge()?;
    let options = AddOptions {
        priority,
        accepted_args,
        key: Some(key),
        owner,
    };
    fire(&hooks, |h| h.add(&hook, php_callback, options))?;
    Ok(Ret::Bool(true))
}

fn remove(call: &Call<'_>) -> Result<Ret, Fail> {
    call.arity(2, 3)?;
    let hook = call.hook_name()?;
    let key = callable_key(call.arg(1))
        .ok_or_else(|| call.type_error(2, "callback", "a valid callback"))?;
    let priority = call.priority(2)?;
    let (hooks, _) = bridge()?;
    fire(&hooks, |h| h.remove_keyed(&hook, &key, priority)).map(Ret::Bool)
}

fn has(call: &Call<'_>) -> Result<Ret, Fail> {
    call.arity(1, 2)?;
    let hook = call.hook_name()?;
    let callback = call.arg(1);
    let (hooks, _) = bridge()?;
    // SAFETY: an argument slot of the live call frame.
    let ty = u32::from(unsafe { convert::type_of(callback) });
    if ty == IS_UNDEF || ty == IS_NULL {
        return fire(&hooks, |h| h.has(&hook)).map(Ret::Bool);
    }
    let key = callable_key(callback)
        .ok_or_else(|| call.type_error(2, "callback", "a valid callback or null"))?;
    fire(&hooks, |h| h.has_keyed(&hook, &key)).map(Ret::Bool)
}

fn run(call: &Call<'_>) -> Result<Ret, Fail> {
    call.arity(1, usize::MAX)?;
    let hook = call.hook_name()?;
    let args = call.values_from(1)?;
    let (hooks, _) = bridge()?;
    fire(&hooks, |h| h.run(&hook, &args))?;
    Ok(Ret::Null)
}

fn filter(call: &Call<'_>) -> Result<Ret, Fail> {
    call.arity(2, usize::MAX)?;
    let hook = call.hook_name()?;
    let mut values = call.values_from(1)?;
    let value = values.remove(0);
    let (hooks, _) = bridge()?;
    fire(&hooks, |h| h.filter(&hook, value, &values)).map(Ret::Value)
}

fn ran(call: &Call<'_>) -> Result<Ret, Fail> {
    call.arity(1, 1)?;
    let hook = call.hook_name()?;
    let (hooks, _) = bridge()?;
    let count = fire(&hooks, |h| h.ran(&hook))?;
    Ok(Ret::Long(i64::try_from(count).unwrap_or(i64::MAX)))
}

fn bridge() -> Result<(SharedHooks, Option<String>), Fail> {
    BRIDGE
        .with(|b| {
            b.borrow()
                .as_ref()
                .map(|bridge| (bridge.hooks.clone(), bridge.owner.clone()))
        })
        .ok_or_else(|| Fail::Hook("the hook registry is not available".to_owned()))
}

/// Runs one registry operation as a firing started from PHP.
fn fire<T>(
    hooks: &SharedHooks,
    op: impl FnOnce(&dyn speedyspec_hooks::Hooks) -> Result<T, HookError>,
) -> Result<T, Fail> {
    let frame = FrameGuard::push();
    let result = op(&**hooks);
    // Dropping a leftover exception queues its release.
    let thrown = frame.pop();
    match result {
        Ok(value) => Ok(value),
        Err(_) if !boundary::can_enter() => Err(Fail::Silent),
        Err(err) => match thrown {
            // The callback that failed was a PHP callback: rethrow its
            // exception unchanged.
            Some((message, object))
                if err.kind == HookErrorKind::Callback && message == err.message =>
            {
                Err(Fail::Rethrow(object))
            }
            _ => Err(Fail::Hook(hook_message(&err))),
        },
    }
}

/// The `HookException` message for a hook failure that is not a PHP
/// callback's own exception: `<hook>: <message>`, plus the owner when known.
fn hook_message(err: &HookError) -> String {
    match &err.owner {
        Some(owner) => format!("{}: {} (registered by {owner})", err.hook, err.message),
        None => format!("{}: {}", err.hook, err.message),
    }
}

/// Pushes a firing frame of the current execution context and pops it again,
/// also when a Rust callback panics. Frames are per context (see
/// `boundary::Engine`): a firing parked in a suspended fiber keeps its frame
/// in that fiber's record, so this always pops its own.
struct FrameGuard {
    popped: bool,
}

impl FrameGuard {
    fn push() -> Self {
        boundary::push_frame();
        Self { popped: false }
    }

    /// Pops the frame and gives back the exception kept in it, if any.
    fn pop(mut self) -> Option<(String, ZvalHandle)> {
        self.popped = true;
        boundary::pop_frame().and_then(|frame| frame.thrown)
    }
}

impl Drop for FrameGuard {
    fn drop(&mut self) {
        if !self.popped {
            drop(boundary::pop_frame());
        }
    }
}

/// The `CallbackError` side of a PHP callback in the registry.
fn run_php_callback(callable: &ZvalHandle, args: &[Value]) -> Result<Value, CallbackError> {
    if !boundary::alive() {
        return Err(CallbackError(
            "the PHP engine has shut down; this PHP callback can no longer run".to_owned(),
        ));
    }
    match boundary::call_php(callable.as_ptr().cast_mut(), args) {
        Ok(value) => Ok(value),
        Err(err) => {
            let message = err.describe();
            if let CallError::Thrown(Thrown::Exception { object, .. }) = err {
                // Keep the original for a PHP caller; with none (fired from
                // Rust), the object is dropped, which queues its release.
                boundary::keep_in_top_frame(message.clone(), object);
            }
            Err(CallbackError(message))
        }
    }
}

/// Sets the return value or throws, inside a guard (both allocate).
fn finish(retval: &mut Zval, result: Result<Ret, Fail>) {
    let retval: *mut zval = retval;
    match result {
        // SAFETY: the engine set `retval` to null before calling us.
        Ok(Ret::Null) => {}
        Ok(Ret::Bool(b)) => {
            // SAFETY: as above; no allocation.
            unsafe {
                (*retval).u1.type_info = if b {
                    ext_php_rs::ffi::IS_TRUE
                } else {
                    ext_php_rs::ffi::IS_FALSE
                }
            };
        }
        Ok(Ret::Long(n)) => {
            // SAFETY: as above; no allocation.
            unsafe {
                (*retval).value.lval = n;
                (*retval).u1.type_info = IS_LONG;
            }
        }
        Ok(Ret::Value(value)) => {
            let written = boundary::guard(|| {
                // SAFETY: as above; `value` is owned by this frame.
                unsafe { convert::write_value(retval, &value) }
            });
            if let Ok(Err(why)) = written {
                // A partly built array may be in `retval`; PHP releases it.
                throw(Fail::Hook(format!(
                    "the filtered value cannot be returned to PHP: {why}"
                )));
            }
        }
        Err(fail) => throw(fail),
    }
}

fn throw(fail: Fail) {
    // SAFETY: reading the engine's class entries for its built-in errors.
    let (ce, message) = unsafe {
        match fail {
            Fail::ArgumentCount(m) => (zend_ce_argument_count_error, m),
            Fail::Type(m) => (zend_ce_type_error, m),
            Fail::Value(m) => (zend_ce_value_error, m),
            Fail::Hook(m) => (EXCEPTION_CLASS.with(Cell::get), m),
            Fail::Rethrow(object) => {
                boundary::rethrow(object);
                return;
            }
            Fail::Silent => return,
        }
    };
    if ce.is_null() {
        return;
    }
    let message = CString::new(message.replace('\0', "\\0")).unwrap_or_default();
    let message_ptr = message.as_ptr();
    let hook_exception = EXCEPTION_CLASS.with(Cell::get);
    let _ = boundary::guard(|| {
        // SAFETY: throwing from inside a native function, with a message
        // owned by the enclosing frame.
        unsafe {
            if ce == hook_exception {
                zend_throw_exception(ce, message_ptr, 0);
            } else {
                zend_throw_error(ce, c"%s".as_ptr(), message_ptr);
            }
        }
    });
}

/// The registry key of a PHP callable, so it can be removed or looked up
/// again without its `CallbackId`: the function name for a string, `Class::method`
/// for a static pair, `spl_object_id::method` for an instance pair, and
/// `spl_object_id` for a closure or invokable object.
fn callable_key(callable: *const zval) -> Option<String> {
    // SAFETY: `callable` is a live, dereferenced argument; each branch reads
    // the union member matching the type.
    unsafe {
        match u32::from(convert::type_of(callable)) {
            IS_STRING => Some(
                String::from_utf8_lossy(convert::str_bytes((*callable).value.str_)).into_owned(),
            ),
            IS_OBJECT => Some((*(*callable).value.obj).handle.to_string()),
            IS_ARRAY => {
                let ht = (*callable).value.arr;
                if (*ht).nNumOfElements != 2 {
                    return None;
                }
                let target = zend_hash_index_find(ht, 0);
                let method = zend_hash_index_find(ht, 1);
                if target.is_null() || method.is_null() {
                    return None;
                }
                let (target, method) = (convert::deref(target), convert::deref(method));
                if u32::from(convert::type_of(method)) != IS_STRING {
                    return None;
                }
                let method = String::from_utf8_lossy(convert::str_bytes((*method).value.str_));
                match u32::from(convert::type_of(target)) {
                    IS_STRING => {
                        let class =
                            String::from_utf8_lossy(convert::str_bytes((*target).value.str_));
                        Some(format!("{class}::{method}"))
                    }
                    IS_OBJECT => Some(format!("{}::{method}", (*(*target).value.obj).handle)),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Arguments

/// The arguments of one native call.
struct Call<'a> {
    ex: &'a ExecuteData,
    /// `Class::method` or `function`, for error messages.
    name: String,
    count: usize,
}

impl<'a> Call<'a> {
    fn new(ex: &'a mut ExecuteData) -> Self {
        // SAFETY: PHP called us with a live frame whose `func` is this
        // function; all members of `This.u2` are the same u32.
        let (name, count) = unsafe {
            let func = ex.func;
            let mut name = String::new();
            if !func.is_null() {
                let scope = (*func).common.scope;
                if !scope.is_null() {
                    name.push_str(&String::from_utf8_lossy(convert::str_bytes((*scope).name)));
                    name.push_str("::");
                }
                let fname = (*func).common.function_name;
                if !fname.is_null() {
                    name.push_str(&String::from_utf8_lossy(convert::str_bytes(fname)));
                }
            }
            (name, ex.This.u2.num_args as usize)
        };
        Self { ex, name, count }
    }

    /// Argument `i` (0-based), dereferenced; an UNDEF zval when not passed.
    fn arg(&self, i: usize) -> *const zval {
        static UNDEF: SyncZval = SyncZval(Zval::new());
        if i >= self.count {
            // `Zval::new()` is null, not undef; callers treat both as absent.
            return &raw const UNDEF.0;
        }
        // SAFETY: `i < num_args`, so the slot is inside the frame.
        match unsafe { self.ex.zend_call_arg(i) } {
            // SAFETY: a live argument slot.
            Some(zv) => unsafe { convert::deref(&raw const *zv) },
            None => &raw const UNDEF.0,
        }
    }

    fn arity(&self, min: usize, max: usize) -> Result<(), Fail> {
        let fail = |bound: &str, n: usize| {
            let plural = if n == 1 { "" } else { "s" };
            Err(Fail::ArgumentCount(format!(
                "{}() expects {bound} {n} argument{plural}, {} given",
                self.name, self.count
            )))
        };
        if self.count < min {
            return fail(if min == max { "exactly" } else { "at least" }, min);
        }
        if self.count > max {
            return fail(if min == max { "exactly" } else { "at most" }, max);
        }
        Ok(())
    }

    fn type_error(&self, position: usize, param: &str, expected: &str) -> Fail {
        Fail::Type(format!(
            "{}(): Argument #{position} (${param}) must be {expected}, {} given",
            self.name,
            type_name(self.arg(position - 1))
        ))
    }

    fn hook_name(&self) -> Result<String, Fail> {
        let zv = self.arg(0);
        // SAFETY: a live argument slot.
        unsafe {
            if u32::from(convert::type_of(zv)) != IS_STRING {
                return Err(self.type_error(1, "hook", "of type string"));
            }
            String::from_utf8(convert::str_bytes((*zv).value.str_).to_vec()).map_err(|_| {
                Fail::Value(format!(
                    "{}(): Argument #1 ($hook) must be valid UTF-8",
                    self.name
                ))
            })
        }
    }

    /// An integer argument, or `default` when it was not passed.
    ///
    /// Accepted whatever the caller's `strict_types`, as WordPress does for
    /// priorities: an int, a float with an integral value, or a numeric
    /// string (PHP's own parsing, surrounding whitespace allowed) with an
    /// integral value. Anything else, including null, bools, fractions and
    /// values outside the 64-bit range, is a `TypeError`.
    fn int_or(&self, i: usize, param: &str, default: i64) -> Result<i64, Fail> {
        let zv = self.arg(i);
        let not_int = || {
            self.type_error(
                i + 1,
                param,
                "an integer (int, or integral float or numeric string)",
            )
        };
        // SAFETY: a live argument slot; each branch reads the union member
        // matching the type.
        unsafe {
            match u32::from(convert::type_of(zv)) {
                IS_UNDEF => Ok(default),
                IS_NULL if i >= self.count => Ok(default),
                IS_LONG => Ok((*zv).value.lval),
                IS_DOUBLE => integral((*zv).value.dval).ok_or_else(not_int),
                IS_STRING => {
                    let bytes = convert::str_bytes((*zv).value.str_);
                    let mut lval: i64 = 0;
                    let mut dval: f64 = 0.0;
                    // The inline is_numeric_string() wrapper only adds a fast
                    // reject for a first byte above '9'.
                    let kind = if bytes.first().is_some_and(|b| *b > b'9') {
                        0
                    } else {
                        _is_numeric_string_ex(
                            bytes.as_ptr().cast(),
                            bytes.len(),
                            &raw mut lval,
                            &raw mut dval,
                            false,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        )
                    };
                    match u32::from(kind) {
                        IS_LONG => Ok(lval),
                        IS_DOUBLE => integral(dval).ok_or_else(not_int),
                        _ => Err(not_int()),
                    }
                }
                _ => Err(not_int()),
            }
        }
    }

    /// A priority: an integer argument (see [`Call::int_or`]). The registry's
    /// priority is 64 bits, so every PHP int fits, `PHP_INT_MIN` and
    /// `PHP_INT_MAX` included.
    fn priority(&self, i: usize) -> Result<i64, Fail> {
        self.int_or(i, "priority", 10)
    }

    /// Arguments from `first` on, as hook values.
    fn values_from(&self, first: usize) -> Result<Vec<Value>, Fail> {
        (first..self.count)
            .map(|i| {
                // SAFETY: a live argument slot.
                unsafe { convert::to_value(self.arg(i)) }.map_err(|why| {
                    Fail::Value(format!(
                        "{}(): Argument #{} cannot be passed to a hook: {why}",
                        self.name,
                        i + 1
                    ))
                })
            })
            .collect()
    }
}

/// `d` as an i64 when it is a whole number within the 64-bit range.
fn integral(d: f64) -> Option<i64> {
    // 2^63 is exactly representable; i64::MAX is not.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    (d.is_finite() && d.fract() == 0.0 && (-LIMIT..LIMIT).contains(&d)).then_some(d as i64)
}

/// A zval in a `static`; only ever read, on the engine's thread.
struct SyncZval(Zval);
// SAFETY: the value is an immutable null zval that owns nothing; sharing a
// pointer to it across threads cannot race.
unsafe impl Sync for SyncZval {}

/// The PHP type name of a value, for error messages.
fn type_name(zv: *const zval) -> &'static str {
    // SAFETY: a live zval.
    match u32::from(unsafe { convert::type_of(zv) }) {
        IS_UNDEF | IS_NULL => "null",
        ext_php_rs::ffi::IS_FALSE | ext_php_rs::ffi::IS_TRUE => "bool",
        IS_LONG => "int",
        IS_DOUBLE => "float",
        IS_STRING => "string",
        IS_ARRAY => "array",
        IS_OBJECT => "object",
        ext_php_rs::ffi::IS_RESOURCE => "resource",
        _ => "mixed",
    }
}
