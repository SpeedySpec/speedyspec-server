//! Small readers over PHP array values, named after the PHP they stand for.

use std::borrow::Cow;

use speedyspec_hooks::{Key, Value};

use crate::array_path;

/// `empty( $value )`.
pub fn empty(value: &Value) -> bool {
    !crate::merge::truthy(value)
}

/// `empty( $array[a][b]… )`: missing counts as empty.
pub fn empty_at(value: &Value, path: &[&str]) -> bool {
    array_path::get(value, path).is_none_or(empty)
}

/// The string a `Str` or `Bytes` value holds.
pub fn as_str(value: &Value) -> Option<Cow<'_, str>> {
    match value {
        Value::Str(s) => Some(Cow::Borrowed(s.as_str())),
        Value::Bytes(b) => Some(String::from_utf8_lossy(b)),
        _ => None,
    }
}

/// `is_string( $array[a][b]… )`, returning the string.
pub fn str_at<'a>(value: &'a Value, path: &[&str]) -> Option<Cow<'a, str>> {
    array_path::get(value, path).and_then(as_str)
}

/// An array key as PHP prints it.
pub fn key_str(key: &Key) -> String {
    match key {
        Key::Int(i) => i.to_string(),
        Key::Str(s) => s.clone(),
    }
}

/// A string-keyed key (`php_key` normalisation).
pub fn key(name: &str) -> Key {
    array_path::php_key(name)
}

/// A PHP string value.
pub fn s(text: impl Into<String>) -> Value {
    Value::Str(text.into())
}

/// A PHP list.
pub fn list(items: Vec<Value>) -> Value {
    Value::Array(
        items
            .into_iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v))
            .collect(),
    )
}

/// A PHP array from key/value pairs.
pub fn map<K: AsRef<str>>(entries: Vec<(K, Value)>) -> Value {
    Value::Array(
        entries
            .into_iter()
            .map(|(k, v)| (key(k.as_ref()), v))
            .collect(),
    )
}

/// The entries of an array value (none for other values).
pub fn entries(value: &Value) -> &[(Key, Value)] {
    match value {
        Value::Array(e) => e,
        _ => &[],
    }
}

/// The entries at a path (none when missing or not an array).
pub fn entries_at<'a>(value: &'a Value, path: &[&str]) -> &'a [(Key, Value)] {
    array_path::get(value, path).map(entries).unwrap_or(&[])
}

/// A path of owned strings as `&str`s.
pub fn path_refs(path: &[String]) -> Vec<&str> {
    path.iter().map(String::as_str).collect()
}
