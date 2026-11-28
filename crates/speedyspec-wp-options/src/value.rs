//! Option values as the `option_value` column holds them (Serialized LOB). The PHP casts that decide them
//! (`php_string`, `php_empty`) live in the options contracts.

use speedyspec_wp_hooks_contracts::HookValue;

use crate::{OptionsError, php_string};

/// The bytes in `option_value`: already `maybe_serialize()`d by the caller.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SerializedValue(Vec<u8>);

impl SerializedValue {
    pub fn new(bytes: Vec<u8>) -> SerializedValue {
        SerializedValue(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The column's text as PHP reads it: a string, its bytes kept when they are not UTF-8.
    pub fn to_value(&self) -> HookValue {
        match String::from_utf8(self.0.clone()) {
            Ok(text) => HookValue::Str(text),
            Err(e) => HookValue::Bytes(e.into_bytes()),
        }
    }

    /// The value as `$wpdb->prepare('%s')` formats it for `add_option`'s `INSERT`.
    pub fn for_insert(value: &HookValue) -> Result<SerializedValue, OptionsError> {
        php_string(value)
            .map(SerializedValue)
            .ok_or_else(|| unserialized(value))
    }

    /// The value as `wpdb::update` formats it: `null` is SQL `NULL` (`None`).
    pub fn for_update(value: &HookValue) -> Result<Option<SerializedValue>, OptionsError> {
        match value {
            HookValue::Null => Ok(None),
            other => SerializedValue::for_insert(other).map(Some),
        }
    }
}

fn unserialized(value: &HookValue) -> OptionsError {
    OptionsError::Serialize(format!("{value:?} must be serialized before it is stored"))
}
