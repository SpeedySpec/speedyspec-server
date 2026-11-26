//! `MetaMapper` (Data Mapper): a meta table row to the meta entity and back. The columns come from the meta type.

use speedyspec_data::{DbValue, Row};
use speedyspec_wp_db_contracts::WpValue;
use speedyspec_wp_meta_contracts::{
    Meta, MetaError, MetaId, MetaKey, MetaType, ObjectId, SerializedValue,
};

pub struct MetaMapper {
    meta_type: MetaType,
}

fn bytes(value: Option<&DbValue>) -> Option<Vec<u8>> {
    value.and_then(DbValue::to_mysql_text)
}

/// A PHP string for `$wpdb`: text when the bytes are UTF-8, bytes otherwise.
fn php_string(raw: &[u8]) -> WpValue {
    match String::from_utf8(raw.to_vec()) {
        Ok(text) => WpValue::Str(text),
        Err(e) => WpValue::Bytes(e.into_bytes()),
    }
}

/// Text when the bytes are UTF-8, bytes otherwise.
fn text_or_bytes(raw: &[u8]) -> DbValue {
    match String::from_utf8(raw.to_vec()) {
        Ok(text) => DbValue::Text(text),
        Err(e) => DbValue::Bytes(e.into_bytes()),
    }
}

impl MetaMapper {
    pub fn new(meta_type: MetaType) -> MetaMapper {
        MetaMapper { meta_type }
    }

    fn id(row: &Row, column: &str) -> Result<u64, MetaError> {
        bytes(row.get(column))
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| MetaError::Db(format!("a meta row has no usable {column}")))
    }

    pub fn to_entity(&self, row: &Row) -> Result<Meta, MetaError> {
        let mid = Self::id(row, self.meta_type.id_column())?;
        let object_id = Self::id(row, &self.meta_type.object_column())?;
        let key = bytes(row.get("meta_key")).unwrap_or_default();
        Ok(Meta {
            mid: MetaId(mid),
            object_id: ObjectId(object_id),
            key: MetaKey::new(key),
            value: bytes(row.get("meta_value")).map(SerializedValue::new),
        })
    }

    /// The columns `add_metadata()` hands `$wpdb->insert()`, in its order.
    pub fn insert_values(
        &self,
        object: ObjectId,
        key: &MetaKey,
        value: Option<&SerializedValue>,
    ) -> Vec<(String, DbValue)> {
        vec![
            (self.meta_type.object_column(), DbValue::UInt(object.0)),
            ("meta_key".to_string(), text_or_bytes(key.as_bytes())),
            (
                "meta_value".to_string(),
                value.map_or(DbValue::Null, |v| text_or_bytes(v.as_bytes())),
            ),
        ]
    }

    /// The data `add_metadata()` hands `$wpdb->insert()`, as PHP values, in its order: the object id, `meta_key`
    /// and `meta_value` (`NULL` for `None`).
    pub fn insert_fields(
        &self,
        object: ObjectId,
        key: &MetaKey,
        value: Option<&SerializedValue>,
    ) -> Vec<(String, WpValue)> {
        vec![
            (
                self.meta_type.object_column(),
                WpValue::Int(i64::try_from(object.0).unwrap_or(i64::MAX)),
            ),
            ("meta_key".to_string(), php_string(key.as_bytes())),
            (
                "meta_value".to_string(),
                value.map_or(WpValue::Null, |v| php_string(v.as_bytes())),
            ),
        ]
    }

    /// A PHP string for `$wpdb`, as [`Self::insert_fields`] writes keys and values.
    pub fn php_string(raw: &[u8]) -> WpValue {
        php_string(raw)
    }
}
