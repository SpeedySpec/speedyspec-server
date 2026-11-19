//! PHP callables as hook callbacks, and their identity.

use std::rc::Rc;

use speedyspec_php::{CallFailure, PhpError};
use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_hooks::{
    Callback, CallbackId, CallbackIdentity, CallbackShape, HookCallback, HookError,
};

/// Marks a [`HookError`] raised because PHP is unwinding (a fatal error or `exit()`): nothing is thrown.
pub struct Unwinding;

/// A PHP callable; calling it is a boundary crossing. It is called as WordPress calls it, through
/// `call_user_func()` (no arguments) or `call_user_func_array()`, so an invalid callable fails with WordPress's error.
pub struct PhpCallable(pub Value);

impl HookCallback for PhpCallable {
    fn call(&self, args: &[Value]) -> Result<Value, HookError> {
        let called = if args.is_empty() {
            speedyspec_php::call_catching(
                &Value::Str("call_user_func".into()),
                std::slice::from_ref(&self.0),
            )
        } else {
            let list = args
                .iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), v.clone()))
                .collect();
            speedyspec_php::call_catching(
                &Value::Str("call_user_func_array".into()),
                &[self.0.clone(), Value::Array(list)],
            )
        };
        called.map_err(hook_error)
    }
}

/// The hook error of a failed PHP call: it carries the thrown object, to be rethrown unchanged.
pub fn hook_error(failure: CallFailure) -> HookError {
    let message = match &failure.error {
        PhpError::Exception { message, .. } => message.clone(),
        other => other.to_string(),
    };
    let payload: Option<Rc<dyn std::any::Any>> = match (failure.exception, &failure.error) {
        (Some(exception), _) => Some(Rc::new(exception)),
        (None, PhpError::Fatal { .. } | PhpError::Exit(_)) => Some(Rc::new(Unwinding)),
        (None, _) => None,
    };
    HookError { message, payload }
}

/// The shape `_wp_filter_build_unique_id()` sees in a PHP callable.
pub fn shape(callable: &Value) -> CallbackShape {
    let at = |entries: &[(Key, Value)], i: i64| {
        entries
            .iter()
            .find(|(k, _)| *k == Key::Int(i))
            .map(|(_, v)| v.clone())
    };
    match callable {
        Value::Str(name) => CallbackShape::Name(name.clone()),
        Value::Bytes(name) => CallbackShape::Name(String::from_utf8_lossy(name).into_owned()),
        Value::Opaque(_) => match speedyspec_php::object_id(callable) {
            Some(id) => CallbackShape::Object(id),
            None => CallbackShape::Invalid,
        },
        Value::Array(entries) => match (at(entries, 0), at(entries, 1)) {
            (Some(target), Some(Value::Str(method))) => match target {
                Value::Str(class) => CallbackShape::Static { class, method },
                object @ Value::Opaque(_) => match speedyspec_php::object_id(&object) {
                    Some(object) => CallbackShape::Method { object, method },
                    None => CallbackShape::Invalid,
                },
                _ => CallbackShape::Invalid,
            },
            _ => CallbackShape::Invalid,
        },
        _ => CallbackShape::Invalid,
    }
}

pub fn unique_id(callable: &Value) -> Option<String> {
    CallbackIdentity::unique_id(&shape(callable))
}

pub fn callback_id(callable: &Value) -> CallbackId {
    CallbackId(unique_id(callable))
}

/// A PHP callable as a registered callback.
pub fn php_callback(callable: Value) -> Callback {
    let id = unique_id(&callable);
    Callback::Foreign {
        value: callable.clone(),
        id,
        run: Rc::new(PhpCallable(callable)),
    }
}
