//! `Options` (Facade): typed reads for Rust code that only wants an option's value.

use std::rc::Rc;

use speedyspec_wp_hooks_contracts::HookValue;

use crate::{OptionsApi, OptionsError, php_empty, php_string};

/// A Rust type an option's value converts into.
pub trait FromOptionValue: Sized {
    fn from_option_value(value: HookValue) -> Option<Self>;
}

impl FromOptionValue for HookValue {
    fn from_option_value(value: HookValue) -> Option<HookValue> {
        Some(value)
    }
}

impl FromOptionValue for String {
    fn from_option_value(value: HookValue) -> Option<String> {
        php_string(&value).map(|b| String::from_utf8_lossy(&b).into_owned())
    }
}

/// A PHP integer, or a numeric string that is an integer (surrounding whitespace allowed, as PHP 8 allows it).
impl FromOptionValue for i64 {
    fn from_option_value(value: HookValue) -> Option<i64> {
        match value {
            HookValue::Int(i) => Some(i),
            HookValue::Str(s) => s
                .trim_matches([' ', '\t', '\n', '\r', '\x0B', '\x0C'])
                .parse()
                .ok(),
            _ => None,
        }
    }
}

/// PHP's truthiness.
impl FromOptionValue for bool {
    fn from_option_value(value: HookValue) -> Option<bool> {
        Some(!php_empty(&value))
    }
}

/// Typed option reads over [`OptionsApi`].
pub struct Options {
    api: Rc<OptionsApi>,
}

impl Options {
    pub fn new(api: Rc<OptionsApi>) -> Options {
        Options { api }
    }

    /// `get_option(name)` as `T`: `None` when it returns `false` (a stored option never does: `false` is stored as
    /// `""`) or when the value does not convert.
    pub fn get<T: FromOptionValue>(&self, name: &str) -> Result<Option<T>, OptionsError> {
        let value = self.api.get_option(name, None)?.read().into_owned();
        if matches!(value, HookValue::Bool(false)) {
            return Ok(None);
        }
        Ok(T::from_option_value(value))
    }
}
