//! Values between PHP and Rust: plain data crosses as neutral values; a value with objects in it (which hooks and
//! abilities may return) stays PHP's, held by handle, and comes back as the same value.
//!
//! The stand-in is a one-key map; plain data that happens to use its key (from a client, say) is escaped on the way
//! in and unescaped on the way out, so it can never be taken for a held value.

use speedyspec_operation_contracts::Value;
use speedyspec_operation_php::convert::{from_php, to_php};
use speedyspec_php_ext::{Key, Value as PhpValue};

use crate::handles;

/// The key of the map that stands for a PHP value held by handle.
const PHP_VALUE: &str = "speedyspec:php-value";
/// The key of the map that wraps plain data using one of these keys.
const ESCAPED: &str = "speedyspec:escaped";

fn reserved(map: &indexmap::IndexMap<String, Value>) -> bool {
    map.contains_key(PHP_VALUE) || map.contains_key(ESCAPED)
}

/// Plain data, with every map that uses a reserved key wrapped.
fn escape(value: Value) -> Value {
    match value {
        Value::List(items) => Value::List(items.into_iter().map(escape).collect()),
        Value::Map(map) => {
            let wrap = reserved(&map);
            let inner = Value::Map(map.into_iter().map(|(k, v)| (k, escape(v))).collect());
            if wrap {
                Value::map([(ESCAPED, inner)])
            } else {
                inner
            }
        }
        other => other,
    }
}

/// The neutral value of a PHP value: the data itself, or a stand-in for a value only PHP can hold.
pub(crate) fn neutral(value: &PhpValue) -> Value {
    match from_php(value) {
        Ok(plain) => escape(plain),
        Err(_) => Value::map([(PHP_VALUE, Value::Int(handles::add(value.clone()) as i64))]),
    }
}

/// The PHP value of a neutral value: the held PHP value for a stand-in, escaped data unwrapped, the data otherwise.
pub(crate) fn php(value: &Value) -> PhpValue {
    match value {
        Value::Map(map) if map.len() == 1 => {
            if let Some(Value::Int(handle)) = map.get(PHP_VALUE) {
                return handles::get(*handle as u64);
            }
            if let Some(Value::Map(inner)) = map.get(ESCAPED) {
                return php_map(inner);
            }
            php_map(map)
        }
        Value::Map(map) => php_map(map),
        Value::List(items) => PhpValue::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), php(v)))
                .collect(),
        ),
        other => to_php(other),
    }
}

fn php_map(map: &indexmap::IndexMap<String, Value>) -> PhpValue {
    PhpValue::Array(
        map.iter()
            .map(|(k, v)| (Key::Str(k.clone()), php(v)))
            .collect(),
    )
}
