//! `get_metadata_by_mid()`, `update_metadata_by_mid()` and `delete_metadata_by_mid()`.
//!
//! These take no slashed input: `meta.php` runs no `wp_unslash()` here.

use speedyspec_php_serialize::{PhpValue, maybe_unserialize};
use speedyspec_wp_hooks_contracts::{HookName, HookValue, Key};
use speedyspec_wp_meta_contracts::{Meta, MetaId, MetaKey, MetaType, MetadataError, ObjectId};

use crate::api::{
    MetadataApi, col, column, group, hv, int, key_value, pv, serialize_for_db, truthy,
};
use crate::{MetaHookNames, sanitize_meta};

/// What `get_metadata_by_mid()` returns.
#[derive(Clone, Debug, PartialEq)]
pub enum ByMidOutcome {
    /// The row; its `meta_value` is still serialized (the caller runs `maybe_unserialize()`).
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
    /// A short-circuit answer from `get_{type}_metadata_by_mid` that is not a row object makes WordPress read
    /// properties off a non-object (warnings, then null columns); the native path takes that as no row.
    fn row(&self, meta_type: &MetaType, mid: MetaId) -> Result<Option<Meta>, MetadataError> {
        Ok(match self.get_metadata_by_mid(meta_type, mid)? {
            ByMidOutcome::Found(meta) => Some(meta),
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
        // `$meta->{$column}` is the column's string.
        let object = meta.object_id;
        let key = key.cloned().unwrap_or(meta.key);
        let subtype = self.subtype(meta_type, object)?;
        // `$_meta_value` is taken before `sanitize_meta()` here, unlike in update_metadata().
        let passed = value.clone();
        let sanitized = sanitize_meta(&*self.d.hooks, &key, value, meta_type.name(), &subtype)?;
        let (serialized, bytes) = serialize_for_db(&sanitized);
        self.action(names.update().as_str(), || {
            vec![int(mid.0), col(object.0), key_value(&key), hv(&passed)]
        })?;
        self.legacy_action(names.legacy_update(), || {
            vec![int(mid.0), col(object.0), key_value(&key), hv(&serialized)]
        })?;
        if repository.update_by_mid(mid, &key, bytes.as_ref())? == 0 {
            return Ok(false);
        }
        self.d.cache.delete(&int(object.0), &group(meta_type))?;
        self.action(names.updated().as_str(), || {
            vec![int(mid.0), col(object.0), key_value(&key), hv(&passed)]
        })?;
        self.legacy_action(names.legacy_updated(), || {
            vec![int(mid.0), col(object.0), key_value(&key), hv(&serialized)]
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
        let object: ObjectId = meta.object_id;
        let value = maybe_unserialize(&column(&meta.value));
        let ids = || HookValue::Array(vec![(Key::Int(0), int(mid.0))]);
        let args = || vec![ids(), int(object.0), key_value(&meta.key), hv(&value)];
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
