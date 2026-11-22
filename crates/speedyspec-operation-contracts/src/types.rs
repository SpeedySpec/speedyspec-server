//! Named resource types and their relations.

use indexmap::IndexMap;

use crate::Property;

/// A named resource type (`post`, `user`). Operations refer to it with a `Ref` schema, so every operation returning a
/// post returns the same type.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeSpec {
    pub name: String,
    pub fields: IndexMap<String, Property>,
    pub relations: Vec<Relation>,
}

/// A field of a type resolved by another operation: `post.author` by `core/get-user`, its input filled from the post.
#[derive(Clone, Debug, PartialEq)]
pub struct Relation {
    pub field: String,
    pub operation: String,
    /// Target operation input field → source type field.
    pub args: IndexMap<String, String>,
}
