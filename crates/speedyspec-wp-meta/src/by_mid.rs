//! `get_metadata_by_mid()`, `update_metadata_by_mid()` and `delete_metadata_by_mid()`.
//!
//! These take no slashed input: `meta.php` runs no `wp_unslash()` here.

use speedyspec_php_serialize::semantics::{get, to_php_int, to_php_string};
use speedyspec_php_serialize::{PhpValue, maybe_unserialize};
use speedyspec_wp_hooks_contracts::{HookName, HookValue, Key};
use speedyspec_wp_meta_contracts::{Meta, MetaId, MetaKey, MetaType, MetadataError, ObjectId};

use crate::api::{
    MetadataApi, col, column, group, hv, int, key_value, pv, serialize_for_db, truthy,
};
use crate::{MetaHookNames, sanitize_meta};

/// The fields of a meta row as `meta.php` reads them off `$meta`.
struct Row {
    /// `$meta->{$column}`, as given (a string from the database).
    object: HookValue,
    /// `$meta->meta_key`.
    key: PhpValue,
    /// `$meta->meta_value`, already unserialized.
    value: PhpValue,
}

impl Row {
    /// `(int) $meta->{$column}`, as an object id (a negative one reads as 0).
    fn object_id(&self) -> ObjectId {
        let id = pv(&self.object).map_or(0, |v| to_php_int(&v));
        ObjectId(u64::try_from(id).unwrap_or(0))
    }

    /// `$meta->meta_key` as the key bytes (`null` is the empty key).
    fn meta_key(&self) -> MetaKey {
        MetaKey::new(to_php_string(&self.key))
    }
}

/// What `get_metadata_by_mid()` returns.
#[derive(Clone, Debug, PartialEq)]
pub enum ByMidOutcome {
    /// The row. Its `meta_value` is the stored, still-serialized bytes: WordPress returns it unserialized, so the PHP
    /// adapter runs `maybe_unserialize()` on it before handing the row to PHP.
    Found(Meta),
    /// `false`: an invalid id, no table, or no row.
    NotFound,
    /// The `get_{type}_metadata_by_mid` filter's non-null answer, returned as it is.
    Filtered(PhpValue),
}

impl MetadataApi {
    /// `get_metadata_by_mid( $meta_type, $meta_id )`.
    pub fn get_metadata_by_mid(
        &self,
        meta_type: &MetaType,
        mid: MetaId,
    ) -> Result<ByMidOutcome, MetadataError> {
        if mid.0 == 0 {
            return Ok(ByMidOutcome::NotFound);
        }
        let Some(repository) = self.d.repositories.for_type(meta_type) else {
            return Ok(ByMidOutcome::NotFound);
        };
        let names = MetaHookNames::new(meta_type);
        let check = self.filter(
            HookName::from(names.get_by_mid_pre().as_str()),
            HookValue::Null,
            || vec![int(mid.0)],
        )?;
        if !matches!(check.read().as_ref(), HookValue::Null) {
            return Ok(ByMidOutcome::Filtered(pv(&check)?));
        }
        Ok(match repository.by_mid(mid)? {
            Some(meta) => ByMidOutcome::Found(meta),
            None => ByMidOutcome::NotFound,
        })
    }

    /// The row `update_metadata_by_mid()` and `delete_metadata_by_mid()` go on with: `if ( $meta )`.
    ///
    /// A truthy `get_{type}_metadata_by_mid` answer is a row too, as in WordPress 7.2: a plugin serving meta from its
    /// own storage returns a `stdClass` row, and `meta.php` reads `$meta->{$column}`, `$meta->meta_key` and
    /// `$meta->meta_value` off it. An object or array answer gives those fields (a missing one is `null`); any other
    /// truthy answer gives all three as `null`, as PHP reads properties off a non-object (with warnings).
    fn row(&self, meta_type: &MetaType, mid: MetaId) -> Result<Option<Row>, MetadataError> {
        Ok(match self.get_metadata_by_mid(meta_type, mid)? {
            ByMidOutcome::Found(meta) => Some(Row {
                object: col(meta.object_id.0),
                key: PhpValue::Str(meta.key.as_bytes().to_vec()),
                // get_metadata_by_mid() unserializes the column it read.
                value: maybe_unserialize(&column(&meta.value)),
            }),
            ByMidOutcome::Filtered(answer) if truthy(&answer) => {
                let fields = match &answer {
                    PhpValue::Object { properties, .. } => properties.as_slice(),
                    PhpValue::Array(items) => items.as_slice(),
                    _ => &[],
                };
                let field = |name: &str| get(fields, name).cloned().unwrap_or(PhpValue::Null);
                Some(Row {
                    object: hv(&field(&meta_type.object_column())),
                    key: field("meta_key"),
                    value: field("meta_value"),
                })
            }
            _ => None,
        })
    }

    /// `update_metadata_by_mid( $meta_type, $meta_id, $meta_value, $meta_key )`; `key` `None` is `false` (keep the
    /// row's key).
    pub fn update_metadata_by_mid(
        &self,
        meta_type: &MetaType,
        mid: MetaId,
        value: PhpValue,
        key: Option<&MetaKey>,
    ) -> Result<bool, MetadataError> {
        if mid.0 == 0 {
            return Ok(false);
        }
        let Some(repository) = self.d.repositories.for_type(meta_type) else {
            return Ok(false);
        };
        let names = MetaHookNames::new(meta_type);
        let check = self.filter(
            HookName::from(names.update_by_mid_pre().as_str()),
            HookValue::Null,
            || {
                vec![
                    int(mid.0),
                    hv(&value),
                    key.map_or(HookValue::Bool(false), key_value),
                ]
            },
        )?;
        if !matches!(check.read().as_ref(), HookValue::Null) {
            return Ok(truthy(&pv(&check)?));
        }
        let Some(meta) = self.row(meta_type, mid)? else {
            return Ok(false);
        };
        // `$object_id = $meta->{$column}`, as read: the column's string, or what a short-circuit row holds.
        let object = meta.object.clone();
        let key = key.cloned().unwrap_or_else(|| meta.meta_key());
        let subtype = self.subtype(meta_type, meta.object_id())?;
        // `$_meta_value` is taken before `sanitize_meta()` here, unlike in update_metadata().
        let passed = value.clone();
        let sanitized = sanitize_meta(&*self.d.hooks, &key, value, meta_type.name(), &subtype)?;
        let (serialized, bytes) = serialize_for_db(&sanitized);
        self.action(names.update().as_str(), || {
            vec![int(mid.0), object.clone(), key_value(&key), hv(&passed)]
        })?;
        self.legacy_action(names.legacy_update(), || {
            vec![int(mid.0), object.clone(), key_value(&key), hv(&serialized)]
        })?;
        if repository.update_by_mid(mid, &key, bytes.as_ref())? == 0 {
            return Ok(false);
        }
        // `wp_cache_delete( $object_id, ... )` answers false for a key it rejects (a short-circuit row without the
        // object column); it never stops the update.
        let _ = self.d.cache.delete(&object, &group(meta_type));
        self.action(names.updated().as_str(), || {
            vec![int(mid.0), object.clone(), key_value(&key), hv(&passed)]
        })?;
        self.legacy_action(names.legacy_updated(), || {
            vec![int(mid.0), object.clone(), key_value(&key), hv(&serialized)]
        })?;
        Ok(true)
    }

    /// `delete_metadata_by_mid( $meta_type, $meta_id )`.
    pub fn delete_metadata_by_mid(
        &self,
        meta_type: &MetaType,
        mid: MetaId,
    ) -> Result<bool, MetadataError> {
        if mid.0 == 0 {
            return Ok(false);
        }
        let Some(repository) = self.d.repositories.for_type(meta_type) else {
            return Ok(false);
        };
        let names = MetaHookNames::new(meta_type);
        let check = self.filter(
            HookName::from(names.delete_by_mid_pre().as_str()),
            HookValue::Null,
            || vec![int(mid.0)],
        )?;
        if !matches!(check.read().as_ref(), HookValue::Null) {
            return Ok(truthy(&pv(&check)?));
        }
        let Some(meta) = self.row(meta_type, mid)? else {
            return Ok(false);
        };
        // `(int) $meta->{$column}`, `$meta->meta_key` and `$meta->meta_value`, as read off the row.
        let object = meta.object_id();
        let ids = || HookValue::Array(vec![(Key::Int(0), int(mid.0))]);
        let args = || vec![ids(), int(object.0), hv(&meta.key), hv(&meta.value)];
        self.action(names.delete().as_str(), args)?;
        self.legacy_action(names.legacy_delete_by_mid().as_deref(), || vec![int(mid.0)])?;
        let result = repository.delete_by_mid(mid)? > 0;
        self.d.cache.delete(&int(object.0), &group(meta_type))?;
        self.action(names.deleted().as_str(), args)?;
        self.legacy_action(names.legacy_deleted_by_mid().as_deref(), || {
            vec![int(mid.0)]
        })?;
        Ok(result)
    }
}
