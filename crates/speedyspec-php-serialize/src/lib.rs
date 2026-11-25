//! PHP's serialize format in Rust (a domain service): `serialize`, `unserialize`, WordPress's `maybe_serialize`
//! family, and conversions to the Hook API's values. No PHP runtime is involved.

mod float;
mod read;
mod value;
mod write;

pub mod semantics;

pub use float::php_float;
pub use read::{UnserializeError, unserialize};
pub use value::{PhpKey, PhpValue};
pub use write::serialize;

mod convert;
mod maybe;

pub use convert::{ConvertError, OpaquePhpValue, from_hook_value, to_hook_value};
pub use maybe::{is_serialized, maybe_serialize, maybe_unserialize};
