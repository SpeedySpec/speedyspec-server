//! Native functions written as plain Rust closures over [`Value`]s.
//!
//! [`function`] builds a PHP function whose arguments arrive converted (objects and closures as `Value::Opaque`) and
//! whose result or [`NativeError`] goes back to PHP. Every one runs through the same dispatcher, which looks the
//! handler up by the function's name and goes through `boundary::native_entry`, so bailouts and panics never cross
//! Rust frames. An area uses it from [`crate::NativeArea::functions`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CString;
use std::rc::Rc;

use ext_php_rs::args::Arg;
use ext_php_rs::builders::FunctionBuilder;
use ext_php_rs::ffi::{
    ext_php_rs_executor_globals, zend_ce_exception, zend_class_entry, zend_hash_str_find,
    zend_throw_exception, zval,
};
use ext_php_rs::flags::DataType;
use ext_php_rs::types::Zval;
use ext_php_rs::zend::ExecuteData;
use speedyspec_hooks::Value;

use crate::boundary;
use crate::convert;

/// How a native function fails in PHP.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeError {
    /// Throws `new $class($message)`; `\Exception` when the class is not declared.
    Throw { class: String, message: String },
    /// Rethrows a PHP exception object (a `Value::Opaque` from [`crate::call_catching`]).
    Rethrow(Value),
    /// PHP is unwinding (a bailout or `exit()`); throw nothing.
    Silent,
}

impl NativeError {
    pub fn throw(class: &str, message: impl Into<String>) -> NativeError {
        NativeError::Throw {
            class: class.to_owned(),
            message: message.into(),
        }
    }
}

/// A native function's body: the arguments it was given, converted.
pub type Handler = Rc<dyn Fn(&[Value]) -> Result<Value, NativeError>>;

thread_local! {
    static HANDLERS: RefCell<HashMap<String, Handler>> = RefCell::new(HashMap::new());
}

/// A PHP function `name` (namespaced names allowed) that takes any arguments and runs `handler`.
pub fn function(
    name: &str,
    handler: impl Fn(&[Value]) -> Result<Value, NativeError> + 'static,
) -> FunctionBuilder<'static> {
    HANDLERS.with(|h| {
        h.borrow_mut()
            .insert(name.to_ascii_lowercase(), Rc::new(handler))
    });
    FunctionBuilder::new(name, dispatch)
        .arg(Arg::new("args", DataType::Mixed).is_variadic())
        .returns(DataType::Mixed, false, true)
}

extern "C" fn dispatch(ex: &mut ExecuteData, retval: &mut Zval) {
    boundary::native_entry(ex, retval, dispatch_body);
}

fn dispatch_body(ex: &mut ExecuteData, retval: &mut Zval) {
    let (name, count) = frame(ex);
    let handler = HANDLERS.with(|h| h.borrow().get(&name.to_ascii_lowercase()).cloned());
    let Some(handler) = handler else {
        throw(NativeError::throw(
            "Error",
            format!("{name}() has no native handler"),
        ));
        return;
    };
    let mut args = Vec::with_capacity(count);
    for i in 0..count {
        // SAFETY: `i < num_args`, so the slot is inside the live frame.
        let Some(zv) = (unsafe { ex.zend_call_arg(i) }) else {
            args.push(Value::Null);
            continue;
        };
        // SAFETY: a live argument slot, dereferenced; the conversion takes its own references.
        match unsafe { convert::to_value(convert::deref(&raw const *zv)) } {
            Ok(value) => args.push(value),
            Err(why) => {
                throw(NativeError::throw(
                    "TypeError",
                    format!(
                        "{name}(): Argument #{} cannot be passed to Rust: {why}",
                        i + 1
                    ),
                ));
                return;
            }
        }
    }
    match handler(&args) {
        Ok(value) => {
            let out: *mut zval = retval;
            let written = boundary::guard(|| {
                // SAFETY: the engine set `retval` to null before calling us; `value` is owned by this frame.
                unsafe { convert::write_value(out, &value) }
            });
            if let Ok(Err(why)) = written {
                throw(NativeError::throw(
                    "Error",
                    format!("{name}(): the result cannot be returned to PHP: {why}"),
                ));
            }
        }
        Err(err) => throw(err),
    }
}

/// The called function's name and argument count.
fn frame(ex: &ExecuteData) -> (String, usize) {
    // SAFETY: PHP called us with a live frame whose `func` is this function; all members of `This.u2` are the same
    // u32.
    unsafe {
        let func = ex.func;
        let mut name = String::new();
        if !func.is_null() {
            let fname = (*func).common.function_name;
            if !fname.is_null() {
                name = String::from_utf8_lossy(convert::str_bytes(fname)).into_owned();
            }
        }
        (name, ex.This.u2.num_args as usize)
    }
}

fn throw(err: NativeError) {
    let (class, message) = match err {
        NativeError::Throw { class, message } => (class, message),
        NativeError::Rethrow(object) => {
            if let Some(handle) = convert::handle_of(&object) {
                // SAFETY: a live handle; the copy takes its own reference, which the engine takes over.
                let copy = unsafe { convert::ZvalHandle::copy_of(handle.as_ptr()) };
                boundary::rethrow(copy);
            }
            return;
        }
        NativeError::Silent => return,
    };
    if !boundary::can_enter() {
        return;
    }
    let ce = find_class(&class);
    let message = CString::new(message.replace('\0', "\\0")).unwrap_or_default();
    let message_ptr = message.as_ptr();
    let _ = boundary::guard(|| {
        // SAFETY: throwing from inside a native function, with a message owned by the enclosing frame.
        unsafe { zend_throw_exception(ce, message_ptr, 0) };
    });
}

/// A declared class by name (no autoloading), or `\Exception`.
fn find_class(name: &str) -> *mut zend_class_entry {
    let key = name.trim_start_matches('\\').to_ascii_lowercase();
    // SAFETY: a request is open, so the class table is live; it holds class entries as IS_PTR zvals.
    unsafe {
        let table = (*ext_php_rs_executor_globals()).class_table;
        let zv = zend_hash_str_find(table, key.as_ptr().cast(), key.len());
        if zv.is_null() {
            zend_ce_exception
        } else {
            (*zv).value.ce
        }
    }
}
