//! Contracts of the unified operation API.
//!
//! Plain value objects and ports, with no dependency on any SpeedySpec crate: the neutral [`Value`] every transport
//! decodes to, [`Schema`]s, named types ([`TypeSpec`]) and their relations, operation descriptors ([`OperationSpec`]),
//! errors and their [`Category`], the [`Caller`] with its [`Identity`] port, the [`ContractMapper`] Data Mapper port,
//! the generic [`RawInput`]/[`RawOutput`] contracts, and the catalogue ports. Implementations live in
//! `speedyspec-operation` and `speedyspec-operation-dispatch`.

// `OperationError` is a value object travelling the cold path; boxing it would buy nothing measurable.
#![allow(clippy::result_large_err)]

mod caller;
mod catalog;
mod error;
mod mapper;
mod operation;
mod raw;
mod schema;
mod types;
mod value;

pub use caller::{Caller, INTERNAL_TRANSPORT, Identity, PermissionCheck};
pub use catalog::{OperationCatalog, TypeCatalog};
pub use error::{Category, OperationError};
pub use mapper::{ContractMapper, Extract, Hydrate};
pub use operation::{
    Action, ContractSpec, ErrorSpec, Kind, Labels, NameError, OperationName, OperationSpec,
    Pagination, Resource,
};
pub use raw::{RawInput, RawOutput};
pub use schema::{Property, Schema, SchemaKind, StringFormat};
pub use types::{Relation, TypeSpec};
pub use value::Value;
