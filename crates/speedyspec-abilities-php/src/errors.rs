//! `WP_Error` to and from operation errors, keeping the code, the message and the data exactly.

use speedyspec_operation_contracts::{Category, OperationError, Value};
use speedyspec_operation_php::convert::{from_php, to_php};
use speedyspec_php_ext::Value as PhpValue;

const ADAPTER: &str = "SpeedySpec\\Abilities\\Internal\\Adapter::";

/// Calls a static method of the PHP adapter; `None` (logged) when it fails.
pub(crate) fn adapter(method: &str, args: &[PhpValue]) -> Option<PhpValue> {
    speedyspec_php::call(&PhpValue::Str(format!("{ADAPTER}{method}")), args)
        .map_err(|err| {
            eprintln!("speedyspec_wordpress: the Abilities adapter's {method} failed: {err}")
        })
        .ok()
}

/// The category of an error from its `status` (as REST reads it), else from WordPress's ability codes.
fn category_of(code: &str, data: &Value) -> Category {
    let status = match data.get("status") {
        Some(Value::Int(s)) => Some(*s),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    };
    match status {
        Some(400) => Category::InvalidArgument,
        Some(401) => Category::Unauthenticated,
        Some(403) => Category::PermissionDenied,
        Some(404) => Category::NotFound,
        Some(409) => Category::Conflict,
        Some(429) => Category::ResourceExhausted,
        Some(503) => Category::Unavailable,
        Some(s) if (400..500).contains(&s) => Category::InvalidArgument,
        Some(_) => Category::Internal,
        None => speedyspec_abilities::errors::wp_error(code, "").category,
    }
}

/// The operation error for a `WP_Error`'s parts.
pub fn from_parts(code: &str, message: &str, data: Value) -> OperationError {
    OperationError::new(code, category_of(code, &data), message).with_details(Value::map([(
        "wp_error",
        Value::map([
            ("code", Value::from(code)),
            ("message", Value::from(message)),
            ("data", data),
        ]),
    )]))
}

/// The operation error a `WP_Error` is, or `None` for any other value.
pub fn from_wp_error(value: &PhpValue) -> Option<OperationError> {
    let PhpValue::Array(parts) = adapter("describe_error", std::slice::from_ref(value))? else {
        return None;
    };
    let text = |at: usize| match parts.get(at).map(|(_, v)| v) {
        Some(PhpValue::Str(s)) => s.clone(),
        _ => String::new(),
    };
    let data = parts
        .get(2)
        .map(|(_, v)| from_php(v).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);
    Some(from_parts(&text(0), &text(1), data))
}

/// The `WP_Error` an operation error is: the one it came from when it came from one, else its code, message and
/// details.
pub fn to_wp_error(error: &OperationError) -> PhpValue {
    let (code, message, data) = match error.details.get("wp_error") {
        Some(wp) => (
            match wp.get("code") {
                Some(Value::String(c)) => c.clone(),
                _ => error.code.clone(),
            },
            match wp.get("message") {
                Some(Value::String(m)) => m.clone(),
                _ => error.message.clone(),
            },
            wp.get("data").cloned().unwrap_or(Value::Null),
        ),
        None => (
            error.code.clone(),
            error.message.clone(),
            (*error.details).clone(),
        ),
    };
    adapter(
        "wp_error",
        &[PhpValue::Str(code), PhpValue::Str(message), to_php(&data)],
    )
    .unwrap_or(PhpValue::Null)
}

/// A result crossing from PHP (`[ 'value', $v ]` or `[ 'error', $code, $message, $data ]`), its value still PHP's.
pub(crate) fn outcome(result: &PhpValue) -> Result<PhpValue, OperationError> {
    let PhpValue::Array(items) = result else {
        return Ok(PhpValue::Null);
    };
    let at = |i: usize| {
        items
            .get(i)
            .map(|(_, v)| v.clone())
            .unwrap_or(PhpValue::Null)
    };
    match at(0) {
        PhpValue::Str(kind) if kind == "error" => {
            let text = |v: PhpValue| match v {
                PhpValue::Str(s) => s,
                _ => String::new(),
            };
            Err(from_parts(
                &text(at(1)),
                &text(at(2)),
                from_php(&at(3)).unwrap_or(Value::Null),
            ))
        }
        _ => Ok(at(1)),
    }
}
