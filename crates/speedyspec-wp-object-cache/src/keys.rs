//! PHP's array-key cast, which the object cache's keys and groups go through.

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_object_cache_contracts::ObjectCacheError;

/// PHP's symbol-table rule: a canonical decimal `i64` string is an integer key.
pub(crate) fn canonical_int(text: &str) -> Option<i64> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if (digits.len() > 1 && digits.starts_with('0')) || text == "-0" {
        return None;
    }
    text.parse().ok()
}

/// `gettype()`.
pub(crate) fn gettype(v: &Value) -> &'static str {
    match v {
        Value::Null => "NULL",
        Value::Bool(_) => "boolean",
        Value::Int(_) => "integer",
        Value::Float(_) => "double",
        Value::Str(_) | Value::Bytes(_) => "string",
        Value::Array(_) => "array",
        Value::Opaque(_) => "object",
    }
}

/// A value used as a PHP array key.
pub fn array_key(value: &Value) -> Result<Key, ObjectCacheError> {
    match value {
        Value::Int(i) => Ok(Key::Int(*i)),
        Value::Str(s) => Ok(canonical_int(s).map_or_else(|| Key::Str(s.clone()), Key::Int)),
        Value::Bool(b) => Ok(Key::Int(i64::from(*b))),
        Value::Null => Ok(Key::Str(String::new())),
        Value::Float(f) if f.is_finite() => Ok(Key::Int(*f as i64)),
        Value::Float(_) => Ok(Key::Int(0)),
        Value::Bytes(_) => Err(ObjectCacheError::Unsupported(
            "a cache key that is not UTF-8".into(),
        )),
        other => Err(ObjectCacheError::IllegalOffset(gettype(other).into())),
    }
}

/// The text of a key, as PHP concatenates it.
pub(crate) fn key_text(key: &Key) -> String {
    match key {
        Key::Int(i) => i.to_string(),
        Key::Str(s) => s.clone(),
    }
}
