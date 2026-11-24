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
    from_original(code, message, data, None)
}

/// The operation error for a `WP_Error`'s parts, holding the original object (when there is one) by handle so it
/// comes back as itself, every code and data of it kept.
fn from_original(
    code: &str,
    message: &str,
    data: Value,
    original: Option<PhpValue>,
) -> OperationError {
    let mut wp = vec![
        ("code", Value::from(code)),
        ("message", Value::from(message)),
        ("data", data.clone()),
    ];
    if let Some(original) = original {
        wp.push(("handle", Value::Int(crate::handles::add(original) as i64)));
    }
    OperationError::new(code, category_of(code, &data), message)
        .with_details(Value::map([("wp_error", Value::map(wp))]))
}

/// The HTTP status an error category answers with, as REST answers it.
fn status_of(category: Category) -> i64 {
    match category {
        Category::InvalidArgument => 400,
        Category::Unauthenticated => 401,
        Category::PermissionDenied => 403,
        Category::NotFound => 404,
        Category::AlreadyExists | Category::Conflict => 409,
        Category::ResourceExhausted => 429,
        Category::Unavailable => 503,
        Category::Internal => 500,
    }
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
    Some(from_original(&text(0), &text(1), data, Some(value.clone())))
}

/// The `WP_Error` an operation error is: the very object it came from when it came from one; WordPress's parts when
/// it was built as a WordPress error; else its code and message with its details and a `status` from its category
/// (an internal error with a generic message and no detail, so nothing internal reaches a client).
pub fn to_wp_error(error: &OperationError) -> PhpValue {
    if let Some(Value::Int(handle)) = error
        .details
        .get("wp_error")
        .and_then(|wp| wp.get("handle"))
    {
        let original = crate::handles::get(*handle as u64);
        if !matches!(original, PhpValue::Null) {
            return original;
        }
    }
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
        None => {
            let status = Value::Int(status_of(error.category));
            if error.category == Category::Internal {
                eprintln!("speedyspec_wordpress: an ability failed: {error}");
                (
                    error.code.clone(),
                    "The request could not be processed.".to_owned(),
                    Value::map([("status", status)]),
                )
            } else {
                let data = match &*error.details {
                    Value::Null => Value::map([("status", status)]),
                    Value::Map(map) => {
                        let mut map = map.clone();
                        map.entry("status".to_owned()).or_insert(status);
                        Value::Map(map)
                    }
                    other => Value::map([("status", status), ("details", other.clone())]),
                };
                (error.code.clone(), error.message.clone(), data)
            }
        }
    };
    adapter(
        "wp_error",
        &[PhpValue::Str(code), PhpValue::Str(message), to_php(&data)],
    )
    .unwrap_or(PhpValue::Null)
}

/// A result crossing from PHP (`[ 'value', $v ]`, `[ 'php', $v ]` for a value with objects, or
/// `[ 'error', $code, $message, $data ]`), its value still PHP's.
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
            let original = match at(4) {
                PhpValue::Null => None,
                original => Some(original),
            };
            Err(from_original(
                &text(at(1)),
                &text(at(2)),
                from_php(&at(3)).unwrap_or(Value::Null),
                original,
            ))
        }
        _ => Ok(at(1)),
    }
}
