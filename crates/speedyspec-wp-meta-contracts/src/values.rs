//! The values of the metadata API: the meta type, keys, ids and stored bytes.

/// A meta type (`post`, `user`, or any custom name), which decides the table and column names.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct MetaType(String);

impl MetaType {
    /// `None` for an empty name, as `add_metadata()` rejects `! $meta_type`.
    pub fn new(name: &str) -> Option<MetaType> {
        if name.is_empty() {
            None
        } else {
            Some(MetaType(name.to_owned()))
        }
    }

    pub fn name(&self) -> &str {
        &self.0
    }

    /// The `$wpdb` property `_get_meta_table()` reads: `{type}meta`.
    pub fn table_suffix(&self) -> String {
        format!("{}meta", self.0)
    }

    /// The object id column: `sanitize_key( "{type}_id" )`.
    pub fn object_column(&self) -> String {
        sanitize_key(&format!("{}_id", self.0))
    }

    /// The meta row id column: `umeta_id` for users, `meta_id` for every other type.
    pub fn id_column(&self) -> &'static str {
        if self.0 == "user" {
            "umeta_id"
        } else {
            "meta_id"
        }
    }

    /// The object cache group: `{type}_meta`.
    pub fn cache_group(&self) -> String {
        format!("{}_meta", self.0)
    }
}

/// `sanitize_key()`: ASCII-lowercase, then drop every byte outside `[a-z0-9_-]`.
fn sanitize_key(raw: &str) -> String {
    raw.bytes()
        .map(|b| b.to_ascii_lowercase())
        .filter(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
        .map(char::from)
        .collect()
}

/// A `meta_key`: bytes, because it may hold anything PHP passes.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct MetaKey(Vec<u8>);

impl MetaKey {
    pub fn new(bytes: impl Into<Vec<u8>>) -> MetaKey {
        MetaKey(bytes.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl From<&str> for MetaKey {
    fn from(key: &str) -> MetaKey {
        MetaKey(key.as_bytes().to_vec())
    }
}

/// The id of the object (post, user, ...) the meta belongs to.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ObjectId(pub u64);

/// The id of a meta row (`meta_id` or `umeta_id`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct MetaId(pub u64);

/// The bytes in `meta_value`: already `maybe_serialize()`d by the caller (Serialized LOB).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SerializedValue(Vec<u8>);

impl SerializedValue {
    pub fn new(bytes: Vec<u8>) -> SerializedValue {
        SerializedValue(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}
