//! Contracts of the unified operation API.
//!
//! Plain value objects and ports, with no dependency on any SpeedySpec crate: the neutral [`Value`] every transport
//! decodes to, operation errors and their [`Category`], and (in later modules) schemas, types, operations, the caller
//! and the catalogues. Implementations live in `speedyspec-operation` and `speedyspec-operation-dispatch`.

mod error;
mod value;

pub use error::{Category, OperationError};
pub use value::Value;
