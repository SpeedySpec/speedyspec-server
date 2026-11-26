//! Neutral values to and from PHP-shaped hook values.

use speedyspec_operation_contracts::{Category, OperationError, Value};
use speedyspec_wp_hooks_contracts::{HookValue, Key};

/// The hook value for `value`. Dates become WordPress's MySQL format (`Y-m-d H:i:s`), lists become arrays keyed
/// `0..n`, maps arrays keyed by string.
pub fn to_hook(value: &Value) -> HookValue {
    match value {
        Value::Null => HookValue::Null,
        Value::Bool(b) => HookValue::Bool(*b),
        Value::Int(i) => HookValue::Int(*i),
        Value::Float(f) => HookValue::Float(*f),
        Value::String(s) => HookValue::Str(s.clone()),
        Value::Binary(bytes) => HookValue::Bytes(bytes.clone()),
        Value::DateTime(dt) => HookValue::Str(dt.format("%Y-%m-%d %H:%M:%S").to_string()),
        Value::List(items) => HookValue::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), to_hook(v)))
                .collect(),
        ),
        Value::Map(map) => HookValue::Array(
            map.iter()
                .map(|(k, v)| (Key::Str(k.clone()), to_hook(v)))
                .collect(),
        ),
    }
}

/// The neutral value for a hook value. An array keyed exactly `0..n` in order (the empty array included) is a list;
/// any other array a map with its integer keys written as decimal strings. A value that cannot leave PHP is refused.
pub fn from_hook(value: &HookValue) -> Result<Value, OperationError> {
    let value = value.read();
    Ok(match value.as_ref() {
        HookValue::Null => Value::Null,
        HookValue::Bool(b) => Value::Bool(*b),
        HookValue::Int(i) => Value::Int(*i),
        HookValue::Float(f) => Value::Float(*f),
        HookValue::Str(s) => Value::String(s.clone()),
        HookValue::Bytes(bytes) => Value::Binary(bytes.clone()),
        HookValue::Array(items) => {
            let is_list = items
                .iter()
                .enumerate()
                .all(|(i, (key, _))| *key == Key::Int(i as i64));
            if is_list {
                Value::List(
                    items
                        .iter()
                        .map(|(_, v)| from_hook(v))
                        .collect::<Result<_, _>>()?,
                )
            } else {
                Value::Map(
                    items
                        .iter()
                        .map(|(key, v)| {
                            let key = match key {
                                Key::Int(i) => i.to_string(),
                                Key::Str(s) => s.clone(),
                            };
                            from_hook(v).map(|v| (key, v))
                        })
                        .collect::<Result<_, _>>()?,
                )
            }
        }
        HookValue::Opaque(_) => {
            return Err(OperationError::new(
                "opaque_hook_value",
                Category::Internal,
                "a hook returned a value that cannot leave PHP",
            ));
        }
    })
}
