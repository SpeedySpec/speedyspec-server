//! Converting recorded values to the Hook API's values, for functions that take PHP values.

#![allow(dead_code)]

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_text_corpus::{Php, PhpKey};

pub fn value(php: &Php) -> Value {
    match php {
        Php::Null => Value::Null,
        Php::Bool(b) => Value::Bool(*b),
        Php::Int(i) => Value::Int(*i),
        Php::Float(f) => Value::Float(*f),
        Php::Str(bytes) => match String::from_utf8(bytes.clone()) {
            Ok(s) => Value::Str(s),
            Err(e) => Value::Bytes(e.into_bytes()),
        },
        Php::Array(pairs) => Value::Array(
            pairs
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        PhpKey::Int(i) => Key::Int(*i),
                        PhpKey::Str(s) => Key::Str(String::from_utf8_lossy(s).into_owned()),
                    };
                    (key, value(v))
                })
                .collect(),
        ),
        other => panic!("not a value a test passes: {other}"),
    }
}

pub fn php(value: &Value) -> Php {
    match value {
        Value::Null => Php::Null,
        Value::Bool(b) => Php::Bool(*b),
        Value::Int(i) => Php::Int(*i),
        Value::Float(f) => Php::Float(*f),
        Value::Str(s) => Php::Str(s.as_bytes().to_vec()),
        Value::Bytes(b) => Php::Str(b.clone()),
        Value::Array(pairs) => Php::Array(
            pairs
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        Key::Int(i) => PhpKey::Int(*i),
                        Key::Str(s) => PhpKey::Str(s.as_bytes().to_vec()),
                    };
                    (key, php(v))
                })
                .collect(),
        ),
        Value::Opaque(_) => panic!("an opaque value"),
    }
}
