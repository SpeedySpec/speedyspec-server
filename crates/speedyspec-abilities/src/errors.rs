//! WordPress's ability errors, as operation errors that round-trip to the same `WP_Error`.

use speedyspec_operation_contracts::{Category, OperationError, Value};

/// The category of a WordPress ability error code.
fn category_of(code: &str) -> Category {
    match code {
        "ability_invalid_input" | "ability_missing_input_schema" => Category::InvalidArgument,
        "ability_invalid_permissions" => Category::PermissionDenied,
        _ => Category::Internal,
    }
}

/// A `WP_Error( $code, $message )` as an operation error, its `wp_error` details holding exactly what the adapter
/// gives back to PHP.
pub fn wp_error(code: &str, message: impl Into<String>) -> OperationError {
    let message = message.into();
    OperationError::new(code, category_of(code), message.clone()).with_details(Value::map([(
        "wp_error",
        Value::map([
            ("code", Value::from(code)),
            ("message", Value::String(message)),
            ("data", Value::Null),
        ]),
    )]))
}

/// WordPress's `ability_invalid_permissions` for `name`.
pub fn invalid_permissions(name: &str) -> OperationError {
    wp_error(
        "ability_invalid_permissions",
        format!("Ability \"{name}\" does not have necessary permission."),
    )
}

/// A hook that failed: the call fails closed, with no detail for the client.
pub fn hook_failed() -> OperationError {
    OperationError::new(
        "ability_hook_failed",
        Category::Internal,
        "The request could not be processed.",
    )
}

/// The reason a validation error gives, as WordPress's `Reason:` text: its first violation (with its path), else its
/// message.
pub fn reason(error: &OperationError) -> String {
    if let Value::List(violations) = &*error.details
        && let Some(first) = violations.first()
    {
        let text = |key: &str| match first.get(key) {
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        };
        let (path, message) = (text("path"), text("message"));
        if !message.is_empty() {
            return if path.is_empty() {
                message
            } else {
                format!("{path}: {message}")
            };
        }
    }
    error.message.clone()
}
