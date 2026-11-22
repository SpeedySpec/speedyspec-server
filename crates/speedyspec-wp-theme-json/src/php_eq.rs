//! PHP's loose `==` over PHP values, as tests and the recorded fixtures compare arrays.

use speedyspec_hooks::{Key, Value};

/// PHP 8's `$a == $b`: arrays are equal when they hold the same key/value pairs in any order, compared loosely;
/// scalars follow PHP 8's comparison rules (numeric strings compare as numbers, `null` as `false` or `""`).
pub fn php_loose_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len()
                && x.iter().all(|(k, v)| {
                    y.iter()
                        .find(|(k2, _)| same_key(k, k2))
                        .is_some_and(|(_, v2)| php_loose_eq(v, v2))
                })
        }
        (Value::Array(_), _) | (_, Value::Array(_)) => {
            matches!((a, b), (Value::Array(x), Value::Bool(t)) | (Value::Bool(t), Value::Array(x)) if x.is_empty() != *t)
        }
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), other) | (other, Value::Bool(x)) => *x == truthy(other),
        (Value::Null, other) | (other, Value::Null) => match other {
            Value::Str(s) => s.is_empty(),
            Value::Bytes(s) => s.is_empty(),
            _ => !truthy(other),
        },
        _ => match (number(a), number(b)) {
            (Some(x), Some(y)) => x == y,
            _ => bytes(a) == bytes(b),
        },
    }
}

fn same_key(a: &Key, b: &Key) -> bool {
    a == b
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Int(i) => *i != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => !(s.is_empty() || s == "0"),
        Value::Bytes(s) => !(s.is_empty() || s == b"0"),
        Value::Array(a) => !a.is_empty(),
        Value::Opaque(_) => true,
    }
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        Value::Str(s) => numeric(s.as_bytes()),
        Value::Bytes(s) => numeric(s),
        _ => None,
    }
}

/// PHP 8's numeric strings: optional leading whitespace, a number, optional trailing whitespace.
fn numeric(s: &[u8]) -> Option<f64> {
    let text = std::str::from_utf8(s).ok()?;
    let trimmed =
        text.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c'));
    if trimmed.is_empty() || trimmed.starts_with('+') && trimmed[1..].starts_with(['+', '-']) {
        return None;
    }
    let ok = trimmed
        .bytes()
        .all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-'));
    if !ok {
        return None;
    }
    trimmed.parse::<f64>().ok()
}

fn bytes(v: &Value) -> Vec<u8> {
    match v {
        Value::Str(s) => s.as_bytes().to_vec(),
        Value::Bytes(b) => b.clone(),
        Value::Int(i) => i.to_string().into_bytes(),
        Value::Float(f) => f.to_string().into_bytes(),
        _ => Vec::new(),
    }
}
