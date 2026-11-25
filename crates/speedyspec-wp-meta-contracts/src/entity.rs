//! The meta row as a plain value that knows nothing of how it is stored.

use crate::{MetaId, MetaKey, ObjectId, SerializedValue};

/// One row of a meta table.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Meta {
    pub mid: MetaId,
    pub object_id: ObjectId,
    pub key: MetaKey,
    /// `None` is SQL `NULL`: the column is nullable and `update_metadata_by_mid()` can write it.
    pub value: Option<SerializedValue>,
}
