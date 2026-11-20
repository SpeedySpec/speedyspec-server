//! The second level's codec: PHP's serialize format (Serialized LOB).

use speedyspec_hooks::Value;
use speedyspec_php_serialize::{
    ConvertError, from_hook_value, serialize, to_hook_value, unserialize,
};

use crate::{CodecError, ValueCodec};

/// PHP's `serialize()` and `unserialize()`, in Rust.
pub struct SerializeCodec;

impl ValueCodec for SerializeCodec {
    fn encode(&self, value: &Value) -> Result<Vec<u8>, CodecError> {
        match from_hook_value(value) {
            Ok(php) => Ok(serialize(&php)),
            Err(ConvertError::Foreign) => Err(CodecError::Refused(
                "an object Rust cannot serialize".into(),
            )),
        }
    }

    fn decode(&self, bytes: &[u8]) -> Result<Value, CodecError> {
        unserialize(bytes)
            .map(|v| to_hook_value(&v))
            .map_err(|e| CodecError::Corrupt(e.to_string()))
    }
}
