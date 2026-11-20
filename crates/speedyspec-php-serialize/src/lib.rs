//! PHP's serialize format in Rust (a domain service): `serialize`, `unserialize`, WordPress's `maybe_serialize`
//! family, and conversions to the Hook API's values. No PHP runtime is involved.

mod float;
mod read;
mod value;
mod write;

pub use float::php_float;
pub use read::{UnserializeError, unserialize};
pub use value::{PhpKey, PhpValue};
pub use write::serialize;
