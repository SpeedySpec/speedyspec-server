//! Converting recorded values to the Hook API's values, for functions that take PHP values.

#![allow(dead_code)]

use speedyspec_hooks::Value;
use speedyspec_wp_text_corpus::Php;

pub fn value(php: &Php) -> Value {
    speedyspec_wp_text_corpus::to_value(php)
}

pub fn php(value: &Value) -> Php {
    speedyspec_wp_text_corpus::from_value(value)
}
