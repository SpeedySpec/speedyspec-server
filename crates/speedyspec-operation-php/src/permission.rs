//! Permission callbacks registered from PHP.
//!
//! An operation's specification lives for the worker; its PHP permission callback lives for one request. The
//! specification holds a [`PhpPermission`] naming the operation, and each request's registration binds that request's
//! callback here.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;

use speedyspec_operation_contracts::{Caller, Category, OperationError, PermissionCheck};
use speedyspec_php_ext::Value as PhpValue;

thread_local! {
    static CALLBACKS: RefCell<HashMap<String, PhpValue>> = RefCell::new(HashMap::new());
}

/// Binds `callable` as the permission callback of `operation` for this request.
pub fn bind(operation: &str, callable: PhpValue) {
    CALLBACKS.with(|c| c.borrow_mut().insert(operation.to_owned(), callable));
}

/// Forgets every callback (the request ended).
pub fn clear() {
    CALLBACKS.with(|c| c.borrow_mut().clear());
}

/// This request's callback for `operation`.
pub fn callback(operation: &str) -> Option<PhpValue> {
    CALLBACKS.with(|c| c.borrow().get(operation).cloned())
}

/// The permission check of an operation registered from PHP: runs this request's callback.
pub struct PhpPermission {
    pub operation: String,
}

impl PermissionCheck for PhpPermission {
    fn check(&self, _caller: &Caller, input: &dyn Any) -> Result<(), OperationError> {
        let callable = callback(&self.operation).ok_or_else(|| missing(&self.operation))?;
        let input = input
            .downcast_ref::<PhpValue>()
            .cloned()
            .unwrap_or(PhpValue::Null);
        let allowed = speedyspec_php::call(&callable, &[PhpValue::Null, input]).map_err(|err| {
            OperationError::new("permission_failed", Category::Internal, err.to_string())
        })?;
        if matches!(allowed, PhpValue::Bool(true)) {
            Ok(())
        } else {
            Err(OperationError::new(
                "permission_denied",
                Category::PermissionDenied,
                "you may not call this operation",
            ))
        }
    }
}

pub(crate) fn missing(operation: &str) -> OperationError {
    OperationError::new(
        "permission_callback_missing",
        Category::Internal,
        format!(
            "{operation} has no permission callback in this request; register it on wp_operations_init"
        ),
    )
}
