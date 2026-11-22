//! The unified operation API specification.
//!
//! Builders for schemas ([`S`]), types and operations; the schema validator; the registries with their registration
//! checks; and document export. Contracts live in `speedyspec-operation-contracts`.
//!
//! Patterns: Builder ([`S`]/[`SchemaBuilder`]) producing Value Objects; Data Mapper for JSON (`export`).

mod error;
pub mod export;
pub mod identity;
mod json;
mod mapper;
mod operation;
mod registry;
mod s;
pub mod testing;
mod types;
pub mod validate;

pub use error::RegistrationError;
pub use mapper::{InputMapper, OutputMapper};
pub use operation::{Operation, OperationBuilder};
pub use registry::Registries;
pub use s::{S, SchemaBuilder, SchemaError};
pub use types::{Type, TypeBuilder};
