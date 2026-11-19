//! PHP callables as hook callbacks, and their identity.

use std::rc::Rc;

use speedyspec_php::{CallFailure, Callable, PhpError};
use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_hooks::{
    Callback, CallbackId, CallbackIdentity, CallbackShape, HookCallback, HookError,
};

/// Marks a [`HookError`] raised because PHP is unwinding (a fatal error or `exit()`): nothing is thrown.
pub struct Unwinding;

/// A PHP callable; calling it is one boundary crossing. It is resolved once per request and called through Zend's
/// fast path ([`Callable`]): the arguments reach it as the zvals PHP passed (nothing is copied) and its result comes
/// back the same way. A callable that cannot be resolved is called as WordPress calls it, through
/// `call_user_func()` / `call_user_func_array()`, so an invalid callable fails with WordPress's error.
pub struct PhpCallable(pub Callable);

impl HookCallback for PhpCallable {
    fn call(&self, args: &[Value]) -> Result<Value, HookError> {
        // The callback sees the store's $wp_current_filter, and its changes come back.
        let store = crate::store_for_callbacks();
        if let Some(store) = &store {
            crate::mirror::sync_out(store.as_ref());
        }
        let called = self.0.call(args);
        if let Some(store) = &store {
            crate::mirror::sync_in(store.as_ref());
        }
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

/// A PHP callable as a registered callback: `callable` is what Rust reads (its identity, what `$wp_filter` shows),
/// `target` the callable to call, best passed through unconverted (the very zval PHP gave).
pub fn php_callback_of(callable: Value, target: Value) -> Callback {
    let id = unique_id(&callable);
    Callback::Foreign {
        value: callable,
        id,
        run: Rc::new(PhpCallable(Callable::new(target))),
    }
}

/// A PHP callable, converted, as a registered callback.
pub fn php_callback(callable: Value) -> Callback {
    php_callback_of(callable.clone(), callable)
}
