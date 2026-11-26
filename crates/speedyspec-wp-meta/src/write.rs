//! The write path: `add_metadata()`, `update_metadata()` and `delete_metadata()`, each in `meta.php`'s order: the
//! checks, `get_object_subtype()`, `wp_unslash()`, `sanitize_meta()`, the short-circuit filter, the repository call,
//! the cache delete, then the actions.
//!
//! `meta.php` itself never touches `last_changed`: WordPress's default filters hook `wp_cache_set_*_last_changed` on
//! the `added_*`/`updated_*`/`deleted_*` actions, so that runs as a callback of the actions fired here.

use speedyspec_php_serialize::semantics::is_empty;
use speedyspec_php_serialize::{PhpValue, maybe_serialize};
use speedyspec_wp_hooks_contracts::{HookName, HookValue, Key};
use speedyspec_wp_meta_contracts::{
    MetaId, MetaKey, MetaType, MetadataError, ObjectId, UpdateOutcome,
};

use crate::api::{
    MetadataApi, col, falsy_key, first, group, hv, identical, int, key_value, pv, serialize_for_db,
    stored, truthy,
};
use crate::unslash::{stripslashes, wp_unslash};
use crate::{MetaHookNames, sanitize_meta};

/// What `add_metadata()` returns.
#[derive(Clone, Debug, PartialEq)]
pub enum AddOutcome {
    /// The new meta id.
    Added(MetaId),
    /// `false`: invalid arguments, no table, a unique key that exists, or a failed insert.
    Failed,
    /// The `add_{type}_metadata` filter's non-null answer, returned as it is.
    Filtered(PhpValue),
}

impl AddOutcome {
    /// As the [`Metadata`](speedyspec_wp_meta_contracts::Metadata) contract reads it: a filtered positive int is an
    /// id; any other filtered answer is `None`.
    pub fn mid(&self) -> Option<MetaId> {
        match self {
            AddOutcome::Added(mid) => Some(*mid),
            AddOutcome::Filtered(PhpValue::Int(i)) if *i > 0 => Some(MetaId(*i as u64)),
            _ => None,
        }
    }

    /// What PHP's caller gets: the id, `false`, or the filter's answer.
    pub fn to_php(&self) -> PhpValue {
        match self {
            AddOutcome::Added(mid) => PhpValue::Int(i64::try_from(mid.0).unwrap_or(i64::MAX)),
            AddOutcome::Failed => PhpValue::Bool(false),
            AddOutcome::Filtered(value) => value.clone(),
        }
    }
}

/// `$meta_ids` as `$wpdb->get_col()` returns them: decimal strings.
fn id_list(ids: &[MetaId]) -> HookValue {
    HookValue::Array(
        ids.iter()
            .enumerate()
            .map(|(i, id)| (Key::Int(i as i64), col(id.0)))
            .collect(),
    )
}

/// `'' !== $v && null !== $v && false !== $v`.
fn has_value(value: &PhpValue) -> bool {
    !matches!(value, PhpValue::Null | PhpValue::Bool(false))
        && !matches!(value, PhpValue::Str(s) if s.is_empty())
}

impl MetadataApi {
    /// `add_metadata( $meta_type, $object_id, $meta_key, $meta_value, $unique )`. The key and value come slashed,
    /// as WordPress expects them.
    pub fn add_metadata(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: &MetaKey,
        value: PhpValue,
        unique: bool,
    ) -> Result<AddOutcome, MetadataError> {
        if falsy_key(key) || object.0 == 0 {
            return Ok(AddOutcome::Failed);
        }
        let Some(repository) = self.d.repositories.for_type(meta_type) else {
            return Ok(AddOutcome::Failed);
        };
        let subtype = self.subtype(meta_type, object)?;
        let key = MetaKey::new(stripslashes(key.as_bytes()));
        let value = wp_unslash(&value);
        let value = sanitize_meta(&*self.d.hooks, &key, value, meta_type.name(), &subtype)?;
        let names = MetaHookNames::new(meta_type);
        let check = self.filter(
            HookName::from(names.add_pre().as_str()),
            HookValue::Null,
            || {
                vec![
                    int(object.0),
                    key_value(&key),
                    hv(&value),
                    HookValue::Bool(unique),
                ]
            },
        )?;
        if !matches!(check.read().as_ref(), HookValue::Null) {
            return Ok(AddOutcome::Filtered(pv(&check)?));
        }
        if unique && repository.count_for(object, &key)? > 0 {
            return Ok(AddOutcome::Failed);
        }
        let (_, bytes) = serialize_for_db(&value);
        self.action(names.add().as_str(), || {
            vec![int(object.0), key_value(&key), hv(&value)]
        })?;
        let Some(mid) = repository.add(object, &key, bytes.as_ref())? else {
            return Ok(AddOutcome::Failed);
        };
        self.d.cache.delete(&int(object.0), &group(meta_type))?;
        self.action(names.added().as_str(), || {
            vec![int(mid.0), int(object.0), key_value(&key), hv(&value)]
        })?;
        Ok(AddOutcome::Added(mid))
    }

    /// `update_metadata( $meta_type, $object_id, $meta_key, $meta_value, $prev_value )`; `prev` `None` is the
    /// default `''`.
    pub fn update_metadata(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        raw_key: &MetaKey,
        passed_value: PhpValue,
        prev: Option<PhpValue>,
    ) -> Result<UpdateOutcome, MetadataError> {
        if falsy_key(raw_key) || object.0 == 0 {
            return Ok(UpdateOutcome::Failed);
        }
        let Some(repository) = self.d.repositories.for_type(meta_type) else {
            return Ok(UpdateOutcome::Failed);
        };
        let subtype = self.subtype(meta_type, object)?;
        let key = MetaKey::new(stripslashes(raw_key.as_bytes()));
        let value = wp_unslash(&passed_value);
        let value = sanitize_meta(&*self.d.hooks, &key, value, meta_type.name(), &subtype)?;
        let prev = prev.unwrap_or(PhpValue::Str(Vec::new()));
        let names = MetaHookNames::new(meta_type);
        let check = self.filter(
            HookName::from(names.update_pre().as_str()),
            HookValue::Null,
            || vec![int(object.0), key_value(&key), hv(&value), hv(&prev)],
        )?;
        if !matches!(check.read().as_ref(), HookValue::Null) {
            return Ok(if truthy(&pv(&check)?) {
                UpdateOutcome::Updated
            } else {
                UpdateOutcome::Unchanged
            });
        }
        if is_empty(&prev)
            && let Some(PhpValue::Array(old)) =
                self.get_metadata_raw(meta_type, object, Some(&key), false)?
            && old.len() == 1
            && identical(&first(&old), &value)
        {
            return Ok(UpdateOutcome::Unchanged);
        }
        let ids = repository.ids_for(object, &key)?;
        if ids.is_empty() {
            return Ok(
                match self.add_metadata(meta_type, object, raw_key, passed_value, false)? {
                    AddOutcome::Added(mid) => UpdateOutcome::Added(mid),
                    AddOutcome::Failed => UpdateOutcome::Failed,
                    AddOutcome::Filtered(v) => match v {
                        PhpValue::Int(i) if i > 0 => UpdateOutcome::Added(MetaId(i as u64)),
                        v if truthy(&v) => UpdateOutcome::Updated,
                        _ => UpdateOutcome::Failed,
                    },
                },
            );
        }
        let (serialized, bytes) = serialize_for_db(&value);
        let previous = (!is_empty(&prev))
            .then(|| stored(&maybe_serialize(&prev)))
            .flatten();
        let fire = |name: &str, legacy: Option<&str>| -> Result<(), MetadataError> {
            for id in &ids {
                self.action(name, || {
                    vec![col(id.0), int(object.0), key_value(&key), hv(&value)]
                })?;
                self.legacy_action(legacy, || {
                    vec![col(id.0), int(object.0), key_value(&key), hv(&serialized)]
                })?;
            }
            Ok(())
        };
        fire(&names.update(), names.legacy_update())?;
        // `$where['meta_value'] = maybe_serialize( $prev_value )` when one was given.
        if repository.update_by_key(object, &key, bytes.as_ref(), previous.as_ref())? == 0 {
            return Ok(UpdateOutcome::Failed);
        }
        self.d.cache.delete(&int(object.0), &group(meta_type))?;
        fire(&names.updated(), names.legacy_updated())?;
        Ok(UpdateOutcome::Updated)
    }

    /// `delete_metadata( $meta_type, $object_id, $meta_key, $meta_value, $delete_all )`; `value` `None` is the
    /// default `''`.
    pub fn delete_metadata(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: &MetaKey,
        value: Option<PhpValue>,
        delete_all: bool,
    ) -> Result<bool, MetadataError> {
        if falsy_key(key) || (object.0 == 0 && !delete_all) {
            return Ok(false);
        }
        let Some(repository) = self.d.repositories.for_type(meta_type) else {
            return Ok(false);
        };
        let key = MetaKey::new(stripslashes(key.as_bytes()));
        let value = wp_unslash(&value.unwrap_or(PhpValue::Str(Vec::new())));
        let names = MetaHookNames::new(meta_type);
        let check = self.filter(
            HookName::from(names.delete_pre().as_str()),
            HookValue::Null,
            || {
                vec![
                    int(object.0),
                    key_value(&key),
                    hv(&value),
                    HookValue::Bool(delete_all),
                ]
            },
        )?;
        if !matches!(check.read().as_ref(), HookValue::Null) {
            return Ok(truthy(&pv(&check)?));
        }
        let serialized = maybe_serialize(&value);
        let wanted = has_value(&serialized)
            .then(|| stored(&serialized))
            .flatten();
        let ids =
            repository.ids_matching(&key, (!delete_all).then_some(object), wanted.as_ref())?;
        if ids.is_empty() {
            return Ok(false);
        }
        let objects = if delete_all {
            repository.objects_with(&key, wanted.as_ref())?
        } else {
            vec![object]
        };
        self.action(names.delete().as_str(), || {
            vec![id_list(&ids), int(object.0), key_value(&key), hv(&value)]
        })?;
        self.legacy_action(names.legacy_delete(), || vec![id_list(&ids)])?;
        if repository.delete_ids(&ids)? == 0 {
            return Ok(false);
        }
        // With delete_all, `$object_ids` come from get_col (strings); the cache reads them as int keys.
        let keys: Vec<HookValue> = objects.iter().map(|o| int(o.0)).collect();
        self.d.cache.delete_multiple(&keys, &group(meta_type))?;
        self.action(names.deleted().as_str(), || {
            vec![id_list(&ids), int(object.0), key_value(&key), hv(&value)]
        })?;
        self.legacy_action(names.legacy_deleted(), || vec![id_list(&ids)])?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_value_is_strict() {
        assert!(!has_value(&PhpValue::Str(Vec::new())));
        assert!(!has_value(&PhpValue::Null));
        assert!(!has_value(&PhpValue::Bool(false)));
        assert!(has_value(&PhpValue::Str(b"0".to_vec())));
        assert!(has_value(&PhpValue::Int(0)));
    }

    #[test]
    fn filtered_add_reads_an_int_as_the_id() {
        assert_eq!(
            AddOutcome::Filtered(PhpValue::Int(4)).mid(),
            Some(MetaId(4))
        );
        assert_eq!(AddOutcome::Filtered(PhpValue::Int(0)).mid(), None);
        assert_eq!(
            AddOutcome::Filtered(PhpValue::Bool(true)).to_php(),
            PhpValue::Bool(true)
        );
    }
}
