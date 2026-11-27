//! The read path: `get_metadata()`, `get_metadata_raw()`, `get_metadata_default()`, `metadata_exists()` and
//! `filter_default_metadata()`, in `meta.php`'s order of checks, hooks and cache calls.

use speedyspec_php_serialize::{PhpKey, PhpValue, maybe_unserialize};
use speedyspec_wp_hooks_contracts::{HookName, HookValue};
use speedyspec_wp_meta_contracts::{MetaKey, MetaType, MetadataError, ObjectId};

use crate::api::{MetadataApi, bytes_value, falsy_key, first, hv, int, php_key, pv, truthy};
use crate::cache::Probe;
use crate::{MetaHookNames, default_for};

/// The hooks a `get_metadata()` may fire: the `get_{type}_metadata` short-circuit and the `default_{type}_metadata`
/// filter. A native path asks the hook registry about these before it stays in Rust.
pub fn read_hooks(meta_type: &MetaType) -> Vec<HookName> {
    let names = MetaHookNames::new(meta_type);
    vec![
        HookName::from(names.get_pre().as_str()),
        HookName::from(names.default().as_str()),
    ]
}

/// The key argument as the hooks see it: `''` when none was given.
fn key_arg(key: Option<&MetaKey>) -> HookValue {
    key.map_or(HookValue::Str(String::new()), |k| bytes_value(k.as_bytes()))
}

impl MetadataApi {
    /// `get_metadata()`: the raw value, or `get_metadata_default()` when it is null.
    pub fn get_metadata(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: Option<&MetaKey>,
        single: bool,
    ) -> Result<PhpValue, MetadataError> {
        if let Some(value) = self.get_metadata_raw(meta_type, object, key, single)? {
            return Ok(value);
        }
        self.metadata_default(meta_type, object, key, single)
    }

    /// `get_metadata_raw()`: `None` where WordPress returns null.
    pub fn get_metadata_raw(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: Option<&MetaKey>,
        single: bool,
    ) -> Result<Option<PhpValue>, MetadataError> {
        if object.0 == 0 {
            return Ok(Some(PhpValue::Bool(false)));
        }
        let names = MetaHookNames::new(meta_type);
        let check = self.filter(
            HookName::from(names.get_pre().as_str()),
            HookValue::Null,
            || {
                vec![
                    int(object.0),
                    key_arg(key),
                    HookValue::Bool(single),
                    HookValue::Str(meta_type.name().to_owned()),
                ]
            },
        )?;
        if !matches!(check.read().as_ref(), HookValue::Null) {
            let check = pv(&check)?;
            let value = match check {
                PhpValue::Array(items) if single => first(&items),
                other => other,
            };
            return Ok(non_null(value));
        }
        let key = key.filter(|k| !falsy_key(k));
        let wanted = key.map(php_key);
        Ok(match self.entry(meta_type, object, wanted.as_ref())? {
            Probe::Whole(whole) => non_null(whole),
            Probe::Slot(None) | Probe::Miss => None,
            Probe::Slot(Some(values)) => non_null(if single {
                single_value(&values)
            } else {
                all_values(values)
            }),
        })
    }

    /// `get_metadata_default()`.
    pub fn get_metadata_default(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: &MetaKey,
        single: bool,
    ) -> Result<PhpValue, MetadataError> {
        self.metadata_default(meta_type, object, Some(key), single)
    }

    pub(crate) fn metadata_default(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: Option<&MetaKey>,
        single: bool,
    ) -> Result<PhpValue, MetadataError> {
        let value = if single {
            PhpValue::Str(Vec::new())
        } else {
            PhpValue::Array(Vec::new())
        };
        let names = MetaHookNames::new(meta_type);
        let filtered = self.filter(HookName::from(names.default().as_str()), hv(&value), || {
            vec![
                int(object.0),
                key_arg(key),
                HookValue::Bool(single),
                HookValue::Str(meta_type.name().to_owned()),
            ]
        })?;
        let value = pv(&filtered)?;
        if !single && !is_numeric_array(&value) {
            return Ok(PhpValue::Array(vec![(PhpKey::Int(0), value)]));
        }
        Ok(value)
    }

    /// `metadata_exists()`.
    pub fn metadata_exists(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: &MetaKey,
    ) -> Result<bool, MetadataError> {
        if object.0 == 0 {
            return Ok(false);
        }
        let names = MetaHookNames::new(meta_type);
        let check = self.filter(
            HookName::from(names.get_pre().as_str()),
            HookValue::Null,
            || {
                vec![
                    int(object.0),
                    bytes_value(key.as_bytes()),
                    HookValue::Bool(true),
                    HookValue::Str(meta_type.name().to_owned()),
                ]
            },
        )?;
        if !matches!(check.read().as_ref(), HookValue::Null) {
            return Ok(truthy(&pv(&check)?));
        }
        let wanted = php_key(key);
        Ok(matches!(
            self.entry(meta_type, object, Some(&wanted))?,
            Probe::Slot(Some(_))
        ))
    }

    /// `filter_default_metadata( $value, $object_id, $meta_key, $single, $meta_type )`, the callback
    /// `register_meta()` adds to `default_{type}_metadata`: the value as it is while installing or when no default
    /// is registered, else the registered default. `get_object_subtype()` (with its filter) runs only when a default
    /// is registered for the key and none is registered for the `''` subtype.
    pub fn filter_default_metadata(
        &self,
        value: PhpValue,
        object: ObjectId,
        key: &MetaKey,
        single: bool,
        meta_type: &MetaType,
    ) -> Result<PhpValue, MetadataError> {
        if self.d.installing.installing() {
            return Ok(value);
        }
        let found = default_for(&*self.d.keys, meta_type, key, single, || {
            self.subtype(meta_type, object)
        })?;
        Ok(found.unwrap_or(value))
    }
}

/// `is_null( $value )` as `None`.
fn non_null(value: PhpValue) -> Option<PhpValue> {
    (!matches!(value, PhpValue::Null)).then_some(value)
}

/// `maybe_unserialize( $meta_cache[ $meta_key ][0] )`.
fn single_value(values: &PhpValue) -> PhpValue {
    match values {
        PhpValue::Array(items) => maybe_unserialize(&first(items)),
        // A string offset: its first byte.
        PhpValue::Str(s) => s
            .first()
            .map_or(PhpValue::Null, |b| PhpValue::Str(vec![*b])),
        _ => PhpValue::Null,
    }
}

/// `array_map( 'maybe_unserialize', $meta_cache[ $meta_key ] )`, keys kept.
fn all_values(values: PhpValue) -> PhpValue {
    match values {
        PhpValue::Array(items) => PhpValue::Array(
            items
                .into_iter()
                .map(|(k, v)| (k, maybe_unserialize(&v)))
                .collect(),
        ),
        // array_map() throws on a non-array; nothing the native read can return.
        _ => PhpValue::Null,
    }
}

/// `wp_is_numeric_array()`: an array with no string keys.
fn is_numeric_array(value: &PhpValue) -> bool {
    matches!(value, PhpValue::Array(items) if items.iter().all(|(k, _)| matches!(k, PhpKey::Int(_))))
}
