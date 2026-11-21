//! PHP values as the rewrite API reads and writes them, with PHP's loose conversions.

use speedyspec_php_ext::{Key, Value};
use speedyspec_routing::Rule;

/// PHP's string conversion of a scalar; arrays and objects become empty.
pub fn text(value: &Value) -> String {
    match value {
        Value::Str(s) => s.clone(),
        Value::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(true) => "1".into(),
        _ => String::new(),
    }
}

/// PHP's truthiness.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::Bool(true) => true,
        Value::Int(i) => *i != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => !s.is_empty() && s != "0",
        Value::Bytes(b) => !b.is_empty() && b.as_slice() != b"0",
        Value::Array(items) => !items.is_empty(),
        Value::Opaque(_) => true,
    }
}

pub fn int(value: &Value) -> i64 {
    match value {
        Value::Int(i) => *i,
        Value::Bool(b) => i64::from(*b),
        Value::Float(f) => *f as i64,
        Value::Str(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

pub fn items(value: &Value) -> Vec<(Key, Value)> {
    match value {
        Value::Array(items) => items.clone(),
        _ => Vec::new(),
    }
}

pub fn key_text(key: &Key) -> String {
    match key {
        Key::Str(s) => s.clone(),
        Key::Int(i) => i.to_string(),
    }
}

/// An array's values, in order.
pub fn list(value: &Value) -> Vec<Value> {
    items(value).into_iter().map(|(_, v)| v).collect()
}

/// `regex => query` pairs.
pub fn rules(value: &Value) -> Vec<Rule> {
    items(value)
        .iter()
        .map(|(k, v)| Rule {
            pattern: key_text(k),
            query: text(v),
        })
        .collect()
}

pub fn rules_value(rules: &[Rule]) -> Value {
    Value::Array(
        rules
            .iter()
            .map(|r| (Key::Str(r.pattern.clone()), Value::Str(r.query.clone())))
            .collect(),
    )
}

pub fn strings_value(values: &[String]) -> Value {
    Value::Array(
        values
            .iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), Value::Str(v.clone())))
            .collect(),
    )
}

pub fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i)
}
