//! `Tree` ↔ PHP values. PHP strings become `Tree::Bytes` (they need not be UTF-8); numeric-string keys go back as
//! integer keys, as PHP itself stores them.

use speedyspec_hooks::Key;
use speedyspec_wp_hooks::HookValue as Value;
use speedyspec_wp_media_contracts::Tree;

fn key(name: &str) -> Key {
    match name.parse::<i64>() {
        Ok(i) if i.to_string() == name => Key::Int(i),
        _ => Key::Str(name.to_owned()),
    }
}

fn string(bytes: &[u8]) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(s) => Value::Str(s.to_owned()),
        Err(_) => Value::Bytes(bytes.to_vec()),
    }
}

/// `tree` as a PHP value.
pub fn to_php(tree: &Tree) -> Value {
    match tree {
        Tree::Null => Value::Null,
        Tree::Bool(b) => Value::Bool(*b),
        Tree::Int(i) => Value::Int(*i),
        Tree::Float(f) => Value::Float(*f),
        Tree::Str(s) => Value::Str(s.clone()),
        Tree::Bytes(b) => string(b),
        Tree::List(l) => Value::Array(
            l.iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), to_php(v)))
                .collect(),
        ),
        Tree::Map(m) => Value::Array(m.iter().map(|(k, v)| (key(k), to_php(v))).collect()),
    }
}

/// A PHP value as a tree; objects and resources become `Null`.
pub fn from_php(value: &Value) -> Tree {
    match &*value.read() {
        Value::Null => Tree::Null,
        Value::Bool(b) => Tree::Bool(*b),
        Value::Int(i) => Tree::Int(*i),
        Value::Float(f) => Tree::Float(*f),
        Value::Str(s) => Tree::Bytes(s.as_bytes().to_vec()),
        Value::Bytes(b) => Tree::Bytes(b.clone()),
        Value::Array(items) => Tree::Map(
            items
                .iter()
                .map(|(k, v)| {
                    let k = match k {
                        Key::Int(i) => i.to_string(),
                        Key::Str(s) => s.clone(),
                    };
                    (k, from_php(v))
                })
                .collect(),
        ),
        _ => Tree::Null,
    }
}
