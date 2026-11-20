//! The second level's codec (filled by the persistent level).

use speedyspec_hooks::Value;

use crate::{CodecError, ValueCodec};

/// PHP's serialize format.
pub struct SerializeCodec;

impl ValueCodec for SerializeCodec {
    fn encode(&self, _value: &Value) -> Result<Vec<u8>, CodecError> {
        Err(CodecError::Refused("no second level yet".into()))
    }
    fn decode(&self, _bytes: &[u8]) -> Result<Value, CodecError> {
        Err(CodecError::Corrupt("no second level yet".into()))
    }
}
