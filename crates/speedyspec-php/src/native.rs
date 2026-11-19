//! Native functions written as plain Rust closures over [`Value`]s.
//!
//! [`function`] builds a PHP function whose arguments arrive converted (objects and closures as `Value::Opaque`) and
//! whose result or [`NativeError`] goes back to PHP. [`raw_function`] builds one that gets its arguments unconverted
//! ([`Args`]): it converts only what it reads and passes the rest through as the zvals PHP gave it, for functions on
//! a hot path. Every one runs through the same dispatcher, which finds the handler by the called function (by name
//! the first time, then by the function itself) and goes through `boundary::native_entry`, so bailouts and panics
//! never cross Rust frames. An area uses them from [`crate::NativeArea::functions`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CString;
use std::rc::Rc;

use ext_php_rs::args::Arg;
use ext_php_rs::builders::FunctionBuilder;
use ext_php_rs::ffi::{
    IS_ARRAY, IS_FALSE, IS_LONG, IS_OBJECT, ext_php_rs_executor_globals, zend_ce_exception,
    zend_class_entry, zend_hash_index_find, zend_hash_str_find, zend_throw_exception, zval,
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

/// A native function's body that reads its arguments itself ([`Args`]).
pub type RawHandler = Rc<dyn Fn(&Args<'_>) -> Result<Value, NativeError>>;

#[derive(Clone)]
enum Body {
    Converted(Handler),
    Raw(RawHandler),
}

thread_local! {
    static HANDLERS: RefCell<HashMap<String, Body>> = RefCell::new(HashMap::new());
    /// The handlers already found, by the called `zend_function` (internal functions live as long as the process).
    static BY_FUNCTION: RefCell<HashMap<usize, Body>> = RefCell::new(HashMap::new());
}

fn register(name: &str, body: Body) -> FunctionBuilder<'static> {
    HANDLERS.with(|h| h.borrow_mut().insert(name.to_ascii_lowercase(), body));
    BY_FUNCTION.with(|h| h.borrow_mut().clear());
    FunctionBuilder::new(name, dispatch)
        .arg(Arg::new("args", DataType::Mixed).is_variadic())
        .returns(DataType::Mixed, false, true)
}

/// A PHP function `name` (namespaced names allowed) that takes any arguments and runs `handler`.
pub fn function(
    name: &str,
    handler: impl Fn(&[Value]) -> Result<Value, NativeError> + 'static,
) -> FunctionBuilder<'static> {
    register(name, Body::Converted(Rc::new(handler)))
}

/// A PHP function `name` that takes any arguments and runs `handler` over them unconverted. What it returns goes
/// back to PHP; a value it passed through ([`Args::pass`]) goes back as the very same zval.
pub fn raw_function(
    name: &str,
    handler: impl Fn(&Args<'_>) -> Result<Value, NativeError> + 'static,
) -> FunctionBuilder<'static> {
    register(name, Body::Raw(Rc::new(handler)))
}

/// The arguments of a [`raw_function`] call, as PHP passed them. Each accessor reads one argument; nothing is
/// converted unless asked for.
pub struct Args<'a> {
    ex: &'a ExecuteData,
    count: usize,
}

impl Args<'_> {
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The `i`th argument's zval, references followed; `None` past the last argument.
    fn slot(&self, i: usize) -> Option<*const zval> {
        if i >= self.count {
            return None;
        }
        // SAFETY: `i < num_args`, so the slot is inside the live frame.
        let zv = unsafe { self.ex.zend_call_arg(i) }?;
        // SAFETY: a live argument slot.
        Some(unsafe { convert::deref(&raw const *zv) })
    }

    /// The `i`th argument converted, for Rust to read (`Value::Null` when it was not passed).
    pub fn value(&self, i: usize) -> Result<Value, NativeError> {
        let Some(zv) = self.slot(i) else {
            return Ok(Value::Null);
        };
        // SAFETY: a live argument slot; the conversion takes its own references.
        unsafe { convert::to_value(zv) }.map_err(|why| {
            NativeError::throw(
                "TypeError",
                format!(
                    "{}(): Argument #{} cannot be passed to Rust: {why}",
                    frame(self.ex).0,
                    i + 1
                ),
            )
        })
    }

    /// The `i`th argument passed through unconverted (`Value::Null` when it was not passed).
    pub fn pass(&self, i: usize) -> Value {
        match self.slot(i) {
            // SAFETY: a live argument slot; the handle takes its own reference.
            Some(zv) => unsafe { convert::pass_through(zv) },
            None => Value::Null,
        }
    }

    /// Every argument from the `i`th on, passed through.
    pub fn pass_from(&self, i: usize) -> Vec<Value> {
        (i..self.count).map(|i| self.pass(i)).collect()
    }

    /// The elements of the `i`th argument, an array, each passed through, in order (empty when it is not an
    /// array).
    pub fn items(&self, i: usize) -> Vec<Value> {
        let Some(zv) = self.slot(i) else {
            return Vec::new();
        };
        // SAFETY: a live argument slot; an array zval's `arr` is its live table.
        unsafe {
            if u32::from(convert::type_of(zv)) != IS_ARRAY {
                return Vec::new();
            }
            convert::array_values((*zv).value.arr)
                .into_iter()
                .map(|slot| convert::pass_through(slot))
                .collect()
        }
    }

    /// The hash PHP keeps for the `i`th argument, a string; `None` for anything else.
    pub fn string_hash(&self, i: usize) -> Option<u64> {
        // SAFETY: a live argument slot.
        self.slot(i)
            .and_then(|zv| unsafe { convert::string_hash(zv) })
    }

    /// The `i`th argument if it is an integer.
    pub fn int(&self, i: usize) -> Option<i64> {
        let zv = self.slot(i)?;
        // SAFETY: a live argument slot.
        unsafe { (u32::from(convert::type_of(zv)) == IS_LONG).then(|| (*zv).value.lval) }
    }

    /// True when the `i`th argument is `false`.
    pub fn is_false(&self, i: usize) -> bool {
        // SAFETY: a live argument slot.
        self.slot(i)
            .is_some_and(|zv| unsafe { u32::from(convert::type_of(zv)) == IS_FALSE })
    }

    /// The object of an `i`th argument that is `[$object]` (an array of one object, at key 0).
    pub fn single_object(&self, i: usize) -> Option<Value> {
        let zv = self.slot(i)?;
        // SAFETY: a live argument slot; an array zval's `arr` is its live table, whose slot at index 0 is live.
        unsafe {
            if u32::from(convert::type_of(zv)) != IS_ARRAY {
                return None;
            }
            let ht = (*zv).value.arr;
            if (*ht).nNumOfElements != 1 {
                return None;
            }
            let first = zend_hash_index_find(ht, 0);
            if first.is_null() {
                return None;
            }
            let first = convert::deref(first);
            (u32::from(convert::type_of(first)) == IS_OBJECT).then(|| convert::pass_through(first))
        }
    }
}

extern "C" fn dispatch(ex: &mut ExecuteData, retval: &mut Zval) {
    boundary::native_entry(ex, retval, dispatch_body);
}

/// The handler of the called function.
fn body_of(ex: &ExecuteData) -> Result<Body, String> {
    let func = ex.func as usize;
    if let Some(body) = BY_FUNCTION.with(|h| h.borrow().get(&func).cloned()) {
        return Ok(body);
    }
    let (name, _) = frame(ex);
    let body = HANDLERS
        .with(|h| h.borrow().get(&name.to_ascii_lowercase()).cloned())
        .ok_or(name)?;
    BY_FUNCTION.with(|h| h.borrow_mut().insert(func, body.clone()));
    Ok(body)
}

fn dispatch_body(ex: &mut ExecuteData, retval: &mut Zval) {
    let body = match body_of(ex) {
        Ok(body) => body,
        Err(name) => {
            throw(NativeError::throw(
                "Error",
                format!("{name}() has no native handler"),
            ));
            return;
        }
    };
    let result = match body {
        Body::Raw(handler) => {
            // SAFETY: all members of `This.u2` are the same u32.
            let count = unsafe { ex.This.u2.num_args } as usize;
            handler(&Args { ex, count })
        }
        Body::Converted(handler) => match converted_args(ex) {
            Ok(args) => handler(&args),
            Err(err) => Err(err),
        },
    };
    write_result(ex, retval, result);
}

/// The arguments of a [`function`], converted.
fn converted_args(ex: &ExecuteData) -> Result<Vec<Value>, NativeError> {
    let (name, count) = frame(ex);
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
                return Err(NativeError::throw(
                    "TypeError",
                    format!(
                        "{name}(): Argument #{} cannot be passed to Rust: {why}",
                        i + 1
                    ),
                ));
            }
        }
    }
    Ok(args)
}

/// Hands a handler's result (or error) to PHP.
fn write_result(ex: &ExecuteData, retval: &mut Zval, result: Result<Value, NativeError>) {
    match result {
        Ok(value) => {
            let out: *mut zval = retval;
            // SAFETY: the engine set `retval` to null before calling us.
            if unsafe { convert::write_without_engine(out, &value) } {
                return;
            }
            let written = boundary::guard(|| {
                // SAFETY: the engine set `retval` to null before calling us; `value` is owned by this frame.
                unsafe { convert::write_value(out, &value) }
            });
            if let Ok(Err(why)) = written {
                let (name, _) = frame(ex);
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
