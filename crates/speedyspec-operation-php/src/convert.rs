//! Neutral values to and from the PHP values that cross the native boundary.
//!
//! The same array rules as the transport hooks adapter: a list becomes an array keyed `0..n`, a map an array keyed by
//! string; an array keyed exactly `0..n` in order (the empty array included) comes back a list, any other array a map
//! with its integer keys written as decimal strings. Dates cross as ISO 8601 strings (`Y-m-d\TH:i:s`), which the PHP
//! mapper turns into `DateTimeImmutable`; binary crosses as bytes. Schema-guided normalization in the dispatcher puts
//! the kinds back (a date string under `date-time`, a string under `binary`, `[]` under an object).

use speedyspec_operation_contracts::{Category, OperationError, Value};
use speedyspec_php_ext::{Key, Value as PhpValue};

/// The ISO 8601 format dates cross in.
pub const DATE_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

/// The PHP value for `value`.
pub fn to_php(value: &Value) -> PhpValue {
    match value {
        Value::Null => PhpValue::Null,
        Value::Bool(b) => PhpValue::Bool(*b),
        Value::Int(i) => PhpValue::Int(*i),
        Value::Float(f) => PhpValue::Float(*f),
        Value::String(s) => PhpValue::Str(s.clone()),
        Value::Binary(bytes) => PhpValue::Bytes(bytes.clone()),
        Value::DateTime(dt) => PhpValue::Str(dt.format(DATE_FORMAT).to_string()),
        Value::List(items) => PhpValue::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), to_php(v)))
                .collect(),
        ),
        Value::Map(map) => PhpValue::Array(
            map.iter()
                .map(|(k, v)| (Key::Str(k.clone()), to_php(v)))
                .collect(),
        ),
    }
}

/// The neutral value for a PHP value. A PHP object (or any value that cannot leave PHP) is refused: value objects are
/// read by the PHP mapper before they reach here.
pub fn from_php(value: &PhpValue) -> Result<Value, OperationError> {
    let value = value.read();
    Ok(match value.as_ref() {
        PhpValue::Null => Value::Null,
        PhpValue::Bool(b) => Value::Bool(*b),
        PhpValue::Int(i) => Value::Int(*i),
        PhpValue::Float(f) => Value::Float(*f),
        PhpValue::Str(s) => Value::String(s.clone()),
        PhpValue::Bytes(bytes) => Value::Binary(bytes.clone()),
        PhpValue::Array(items) => {
            let is_list = items
                .iter()
                .enumerate()
                .all(|(i, (key, _))| *key == Key::Int(i as i64));
            if is_list {
                Value::List(
                    items
                        .iter()
                        .map(|(_, v)| from_php(v))
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
                            from_php(v).map(|v| (key, v))
                        })
                        .collect::<Result<_, _>>()?,
                )
            }
        }
        PhpValue::Opaque(_) => {
            return Err(OperationError::new(
                "php_object_not_allowed",
                Category::Internal,
                "a PHP object reached a place that takes plain values",
            ));
        }
    })
}
