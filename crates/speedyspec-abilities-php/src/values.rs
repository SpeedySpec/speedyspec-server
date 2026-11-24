//! Values between PHP and Rust: plain data crosses as neutral values; a value with objects in it (which hooks and
//! abilities may return) stays PHP's, held by handle, and comes back as the same value.

use speedyspec_operation_contracts::Value;
use speedyspec_operation_php::convert::{from_php, to_php};
use speedyspec_php_ext::Value as PhpValue;

use crate::handles;

/// The key of the map that stands for a PHP value held by handle.
const PHP_VALUE: &str = "speedyspec:php-value";

/// The neutral value of a PHP value: the data itself, or a stand-in for a value only PHP can hold.
pub(crate) fn neutral(value: &PhpValue) -> Value {
    match from_php(value) {
        Ok(plain) => plain,
        Err(_) => Value::map([(PHP_VALUE, Value::Int(handles::add(value.clone()) as i64))]),
    }
}

/// The PHP value of a neutral value: the held PHP value for a stand-in, the data otherwise.
pub(crate) fn php(value: &Value) -> PhpValue {
    if let Value::Map(map) = value
        && map.len() == 1
        && let Some(Value::Int(handle)) = map.get(PHP_VALUE)
    {
        return handles::get(*handle as u64);
    }
    to_php(value)
}
