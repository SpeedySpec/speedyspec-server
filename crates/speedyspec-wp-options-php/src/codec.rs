//! `PhpCodec`: `maybe_serialize()` and `maybe_unserialize()` as `RustCodec` does them, except that a value holding a
//! PHP object is serialized and unserialized by PHP, and objects are cloned by PHP.

use speedyspec_php_ext::Value;
use speedyspec_php_serialize::is_serialized;
use speedyspec_wp_options::{OptionsError, RustCodec, ValueCodec};

use crate::adapters::call;

pub struct PhpCodec;

fn holds_object(value: &Value) -> bool {
    match value {
        Value::Opaque(_) => true,
        Value::Array(items) => items.iter().any(|(_, v)| holds_object(v)),
        _ => false,
    }
}

fn bytes(value: &Value) -> Option<&[u8]> {
    match value {
        Value::Str(s) => Some(s.as_bytes()),
        Value::Bytes(b) => Some(b),
        _ => None,
    }
}

impl ValueCodec for PhpCodec {
    fn maybe_serialize(&self, value: &Value) -> Result<Value, OptionsError> {
        if holds_object(value) {
            return call("serialize", std::slice::from_ref(value));
        }
        RustCodec.maybe_serialize(value)
    }

    fn maybe_unserialize(&self, value: Value) -> Result<Value, OptionsError> {
        let Some(data) = bytes(&value) else {
            return Ok(value);
        };
        if !is_serialized(data, true) {
            return Ok(value);
        }
        let rust = RustCodec.maybe_unserialize(value.clone())?;
        if holds_object(&rust) {
            // Objects are PHP's: let PHP build them.
            return call("SpeedySpec\\WpOptions\\unserialize_value", &[value]);
        }
        Ok(rust)
    }

    fn clone_value(&self, value: Value) -> Value {
        if matches!(value, Value::Opaque(_)) {
            return call(
                "SpeedySpec\\WpOptions\\clone_value",
                std::slice::from_ref(&value),
            )
            .unwrap_or(value);
        }
        value
    }
}
