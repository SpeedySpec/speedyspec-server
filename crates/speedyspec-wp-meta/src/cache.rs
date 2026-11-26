//! `update_meta_cache()` (WordPress 7.2-RC2) and the reads of one object's `{type}_meta` entry.
//!
//! An entry is `array( meta_key => array( meta_value, … ) )` of the stored strings, the shape PHP writes, so an entry
//! another PHP process wrote is read as is (Review Focus 4). A key that is not UTF-8 cannot be a hook array key, so
//! such an entry travels as one opaque PHP value.

use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_hooks_contracts::{HookName, HookValue, Key};
use speedyspec_wp_meta_contracts::{MetaType, MetadataError, ObjectId};

use crate::MetaHookNames;
use crate::api::{MetadataApi, column, group, hv, int, lookup, pv, truthy};

/// What one object's cache entry gave a read.
pub(crate) enum Probe {
    /// `! $meta_cache || ! is_array( $meta_cache )`: reload through `update_meta_cache()`.
    Miss,
    /// The whole entry (no key asked).
    Whole(PhpValue),
    /// `$meta_cache[ $meta_key ]`: `None` when it is not set (missing or null).
    Slot(Option<PhpValue>),
}

/// The hook array key for a PHP key; `None` when it cannot be one (not UTF-8).
fn hook_key(key: &PhpKey) -> Option<Key> {
    match key {
        PhpKey::Int(i) => Some(Key::Int(*i)),
        PhpKey::Str(s) => std::str::from_utf8(s).ok().map(|s| Key::Str(s.to_owned())),
    }
}

/// Reads an entry without copying more of it than the read needs.
pub(crate) fn probe(
    entry: Option<&HookValue>,
    key: Option<&PhpKey>,
) -> Result<Probe, MetadataError> {
    let Some(entry) = entry else {
        return Ok(Probe::Miss);
    };
    let read = entry.read();
    match &*read {
        HookValue::Array(items) if items.is_empty() => Ok(Probe::Miss),
        HookValue::Array(items) => match key {
            None => Ok(Probe::Whole(pv(&read)?)),
            Some(key) => {
                let slot = hook_key(key)
                    .and_then(|k| items.iter().find(|(ik, _)| *ik == k).map(|(_, v)| v))
                    .filter(|v| !matches!(v, HookValue::Null));
                Ok(Probe::Slot(slot.map(pv).transpose()?))
            }
        },
        HookValue::Opaque(_) => match pv(&read)? {
            PhpValue::Array(items) if items.is_empty() => Ok(Probe::Miss),
            whole @ PhpValue::Array(_) => Ok(from_whole(whole, key)),
            _ => Ok(Probe::Miss),
        },
        _ => Ok(Probe::Miss),
    }
}

/// A probe of an entry already read whole.
pub(crate) fn from_whole(whole: PhpValue, key: Option<&PhpKey>) -> Probe {
    match key {
        None => Probe::Whole(whole),
        Some(key) => Probe::Slot(slot_of(&whole, key)),
    }
}

/// `isset( $entry[ $key ] )`'s value.
pub(crate) fn slot_of(entry: &PhpValue, key: &PhpKey) -> Option<PhpValue> {
    match entry {
        PhpValue::Array(items) => lookup(items, key)
            .filter(|v| !matches!(v, PhpValue::Null))
            .cloned(),
        _ => None,
    }
}

/// Whether a cached value is an array, as `is_array()` reads it (a PHP-written array may arrive opaque).
fn is_array(value: &HookValue) -> Result<bool, MetadataError> {
    Ok(match &*value.read() {
        HookValue::Array(_) => true,
        HookValue::Opaque(_) => matches!(pv(value)?, PhpValue::Array(_)),
        _ => false,
    })
}

impl MetadataApi {
    /// `update_meta_cache( $meta_type, $object_ids )`.
    ///
    /// `None` where WordPress returns false (no ids, no table, or a short-circuit that is false);
    /// `Some(Bool(true))` for a truthy short-circuit; otherwise the cache array by object id.
    pub fn update_meta_cache(
        &self,
        meta_type: &MetaType,
        objects: &[ObjectId],
    ) -> Result<Option<PhpValue>, MetadataError> {
        if objects.is_empty() {
            return Ok(None);
        }
        let Some(repository) = self.d.repositories.for_type(meta_type) else {
            return Ok(None);
        };
        let names = MetaHookNames::new(meta_type);
        let ids = || {
            HookValue::Array(
                objects
                    .iter()
                    .enumerate()
                    .map(|(i, o)| (Key::Int(i as i64), int(o.0)))
                    .collect(),
            )
        };
        let check = self.filter(
            HookName::from(names.cache_pre().as_str()),
            HookValue::Null,
            || vec![ids()],
        )?;
        if !matches!(check.read().as_ref(), HookValue::Null) {
            return Ok(truthy(&pv(&check)?).then_some(PhpValue::Bool(true)));
        }
        let group = group(meta_type);
        let keys: Vec<HookValue> = objects.iter().map(|o| int(o.0)).collect();
        let cached = self.d.cache.get_multiple(&keys, &group, false)?;
        let mut non_cached: Vec<i64> = Vec::new();
        let mut invalid: Vec<HookValue> = Vec::new();
        let mut cache: Vec<(PhpKey, PhpValue)> = Vec::new();
        for (key, value) in cached {
            let id = match key {
                Key::Int(i) => i,
                Key::Str(s) => s.parse().unwrap_or(0),
            };
            if matches!(value.read().as_ref(), HookValue::Bool(false)) {
                non_cached.push(id);
            } else if !is_array(&value)? {
                non_cached.push(id);
                invalid.push(HookValue::Int(id));
            } else {
                cache.push((PhpKey::Int(id), pv(&value)?));
            }
        }
        if !invalid.is_empty() {
            self.d.cache.delete_multiple(&invalid, &group)?;
        }
        if non_cached.is_empty() {
            return Ok(Some(PhpValue::Array(cache)));
        }
        let wanted: Vec<ObjectId> = non_cached
            .iter()
            .map(|id| ObjectId(u64::try_from(*id).unwrap_or(0)))
            .collect();
        for row in repository.for_objects(&wanted)? {
            let id = PhpKey::Int(i64::try_from(row.object_id.0).unwrap_or(i64::MAX));
            let key = crate::api::php_key(&row.key);
            let slot = match cache.iter().position(|(k, _)| *k == id) {
                Some(at) => at,
                None => {
                    cache.push((id.clone(), PhpValue::Array(Vec::new())));
                    cache.len() - 1
                }
            };
            let entry = &mut cache[slot].1;
            if !matches!(entry, PhpValue::Array(_)) {
                *entry = PhpValue::Array(Vec::new());
            }
            let PhpValue::Array(keys) = entry else {
                unreachable!()
            };
            let at = match keys.iter().position(|(k, _)| *k == key) {
                Some(at) => at,
                None => {
                    keys.push((key, PhpValue::Array(Vec::new())));
                    keys.len() - 1
                }
            };
            let values = &mut keys[at].1;
            if !matches!(values, PhpValue::Array(_)) {
                *values = PhpValue::Array(Vec::new());
            }
            let PhpValue::Array(list) = values else {
                unreachable!()
            };
            let next = list
                .iter()
                .filter_map(|(k, _)| match k {
                    PhpKey::Int(i) if *i >= 0 => Some(i + 1),
                    _ => None,
                })
                .max()
                .unwrap_or(0);
            list.push((PhpKey::Int(next), column(&row.value)));
        }
        let mut data: Vec<(Key, HookValue)> = Vec::new();
        for id in &non_cached {
            let key = PhpKey::Int(*id);
            if lookup(&cache, &key).is_none() {
                cache.push((key.clone(), PhpValue::Array(Vec::new())));
            }
            let entry = lookup(&cache, &key).expect("just set");
            if !data.iter().any(|(k, _)| *k == Key::Int(*id)) {
                data.push((Key::Int(*id), hv(entry)));
            }
        }
        self.d.cache.add_multiple(data, &group, 0)?;
        Ok(Some(PhpValue::Array(cache)))
    }

    /// `$meta_cache = update_meta_cache( .. ); $meta_cache = $meta_cache[ $object_id ] ?? null;`
    pub(crate) fn reload(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
    ) -> Result<PhpValue, MetadataError> {
        let id = PhpKey::Int(i64::try_from(object.0).unwrap_or(i64::MAX));
        Ok(match self.update_meta_cache(meta_type, &[object])? {
            Some(PhpValue::Array(items)) => lookup(&items, &id).cloned().unwrap_or(PhpValue::Null),
            _ => PhpValue::Null,
        })
    }

    /// One object's entry read through `get_with`, reloaded on a miss.
    pub(crate) fn entry(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: Option<&PhpKey>,
    ) -> Result<Probe, MetadataError> {
        let group = group(meta_type);
        let probed = speedyspec_wp_object_cache_contracts::get_with(
            &*self.d.cache,
            &int(object.0),
            &group,
            false,
            |entry| probe(entry, key),
        )??;
        match probed {
            Probe::Miss => Ok(from_whole(self.reload(meta_type, object)?, key)),
            hit => Ok(hit),
        }
    }
}
