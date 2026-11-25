//! WordPress's metadata API contracts: the meta values, the entity and its errors.

#[cfg(feature = "conformance")]
pub mod conformance;
mod entity;
mod error;
mod ports;
mod values;

pub use entity::Meta;
pub use error::MetaError;
pub use ports::{
    Callables, DefaultSchemaCheck, MetaRepositories, MetaRepository, ObjectSubtypes,
    RevisionSupport,
};
pub use values::{MetaId, MetaKey, MetaType, ObjectId, SerializedValue};
