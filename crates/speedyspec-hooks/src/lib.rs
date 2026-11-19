//! Hook contract for speedyspec. Contains no storage: backends implement [`Hooks`].

#[cfg(feature = "conformance")]
pub mod conformance;
mod contract;
mod value;

pub use contract::*;
pub use value::{Key, OpaqueReader, Value, set_opaque_reader};
