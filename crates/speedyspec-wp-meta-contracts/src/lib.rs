//! WordPress's metadata API contracts: the meta values, the entity and its errors.

mod entity;
mod error;
mod values;

pub use entity::Meta;
pub use error::MetaError;
pub use values::{MetaId, MetaKey, MetaType, ObjectId, SerializedValue};
