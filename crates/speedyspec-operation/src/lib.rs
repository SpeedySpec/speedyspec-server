//! The unified operation API specification.
//!
//! Builders for schemas ([`S`]), types and operations; the schema validator; the registries with their registration
//! checks; and document export. Contracts live in `speedyspec-operation-contracts`.
//!
//! Patterns: Builder ([`S`]/[`SchemaBuilder`]) producing Value Objects; Data Mapper for JSON (`export`).

pub mod export;
mod json;
mod s;
pub mod validate;

pub use s::{S, SchemaBuilder, SchemaError};
