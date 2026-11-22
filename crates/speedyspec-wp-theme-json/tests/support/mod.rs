//! Helpers shared by the integration tests.

#![allow(dead_code)]

use speedyspec_hooks::{Key, Value};

/// A PHP value from JSON: objects become string-keyed arrays (key order does not matter to PHP's `==`), lists
/// become lists, integers stay integers.
pub fn json(text: &str) -> Value {
    from_json(&serde_json::from_str(text).expect("valid JSON"))
}

fn from_json(j: &serde_json::Value) -> Value {
    match j {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(Value::Int)
            .unwrap_or_else(|| Value::Float(n.as_f64().unwrap())),
        serde_json::Value::String(s) => Value::Str(s.clone()),
        serde_json::Value::Array(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), from_json(v)))
                .collect(),
        ),
        serde_json::Value::Object(o) => {
            Value::Array(o.iter().map(|(k, v)| (php_key(k), from_json(v))).collect())
        }
    }
}

/// PHP's array key normalisation: a canonical decimal integer string is an integer key.
pub fn php_key(k: &str) -> Key {
    match k.parse::<i64>() {
        Ok(i) if i.to_string() == k => Key::Int(i),
        _ => Key::Str(k.to_owned()),
    }
}

/// Asserts PHP's `==` between two values, printing both on failure.
#[track_caller]
pub fn assert_php_eq(actual: &Value, expected: &Value) {
    assert!(
        speedyspec_wp_theme_json::php_loose_eq(actual, expected),
        "actual:   {actual:?}\nexpected: {expected:?}"
    );
}

/// Asserts a string result is exactly WordPress's (byte for byte); other values compare with PHP's `==`.
#[track_caller]
pub fn assert_php_same(actual: &Value, expected: &Value) {
    match (actual, expected) {
        (Value::Str(a), Value::Str(e)) => assert_eq!(a, e),
        _ => assert_php_eq(actual, expected),
    }
}

/// WordPress's `null` or a string.
pub fn opt(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::Str)
}

/// A PHP string value.
pub fn st(value: impl Into<String>) -> Value {
    Value::Str(value.into())
}
