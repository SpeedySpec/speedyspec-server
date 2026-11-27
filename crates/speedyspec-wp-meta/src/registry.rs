//! `$wp_meta_keys` (Registry): the keys `register_meta()` stored, and what `unregister_meta_key()`,
//! `registered_meta_key_exists()`, `get_registered_meta_keys()` and `filter_default_metadata()` read from it.
//! Ported from WordPress 7.1.5's `wp-includes/meta.php`.

use indexmap::IndexMap;
use speedyspec_php_serialize::semantics::{is_integer_key, isset};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_meta_contracts::{Callables, MetaKey, MetaKeys, MetaType, RegisteredMetaKey};

use crate::hook_names::MetaHookNames;

type Keys = IndexMap<MetaKey, RegisteredMetaKey>;

/// Object type, then subtype, then key, each in insertion order as PHP arrays keep them.
#[derive(Clone, Debug, Default)]
pub struct MetaKeyRegistry {
    types: IndexMap<String, IndexMap<String, Keys>>,
}

impl MetaKeyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// `$wp_meta_keys[t][s][k] = $args`: a key already there keeps its position.
    pub fn insert(&mut self, key: RegisteredMetaKey) {
        self.types
            .entry(key.object_type.clone())
            .or_default()
            .entry(key.subtype.clone())
            .or_default()
            .insert(key.key.clone(), key);
    }

    /// `unregister_meta_key()`'s storage half: the removed entry, then the emptied subtype and the emptied type go
    /// too. Its args let the caller `remove_filter` their callbacks (see [`filters_to_remove`]).
    pub fn remove(
        &mut self,
        object_type: &str,
        key: &MetaKey,
        subtype: &str,
    ) -> Option<RegisteredMetaKey> {
        let subtypes = self.types.get_mut(object_type)?;
        let keys = subtypes.get_mut(subtype)?;
        let removed = keys.shift_remove(key)?;
        if keys.is_empty() {
            subtypes.shift_remove(subtype);
        }
        if subtypes.is_empty() {
            self.types.shift_remove(object_type);
        }
        Some(removed)
    }

    /// `registered_meta_key_exists()`.
    pub fn exists(&self, object_type: &str, key: &MetaKey, subtype: &str) -> bool {
        self.level(object_type, subtype)
            .is_some_and(|keys| keys.contains_key(key))
    }

    /// Every entry, type by type and subtype by subtype, in insertion order (the whole `$wp_meta_keys`).
    pub fn iter(&self) -> impl Iterator<Item = &RegisteredMetaKey> {
        self.types
            .values()
            .flat_map(|subtypes| subtypes.values())
            .flat_map(|keys| keys.values())
    }

    fn level(&self, object_type: &str, subtype: &str) -> Option<&Keys> {
        self.types.get(object_type)?.get(subtype)
    }
}

/// The read port (Registry): `get_registered_meta_keys()` is empty for an unknown type or subtype, in insertion order.
impl MetaKeys for MetaKeyRegistry {
    fn keys(&self, object_type: &str, subtype: &str) -> Vec<RegisteredMetaKey> {
        self.level(object_type, subtype)
            .map(|keys| keys.values().cloned().collect())
            .unwrap_or_default()
    }

    fn find(&self, object_type: &str, subtype: &str, key: &MetaKey) -> Option<RegisteredMetaKey> {
        self.level(object_type, subtype)?.get(key).cloned()
    }
}

/// The registered keys with the one read [`MetaKeys`] lacks: which subtypes a type has registered keys under.
///
/// `filter_default_metadata()` walks every subtype of `$wp_meta_keys[ $meta_type ]` to find the key's defaults before
/// it asks for the object's subtype, so the read port needs the subtypes. Contracts 0.3.0's [`MetaKeys`] cannot list
/// them; this extension belongs in the contracts' next release.
pub trait MetaKeyIndex: MetaKeys {
    /// The subtypes of `$wp_meta_keys[ $object_type ]` in insertion order, empty for an unknown type.
    fn subtypes(&self, object_type: &str) -> Vec<String>;
}

impl MetaKeyIndex for MetaKeyRegistry {
    fn subtypes(&self, object_type: &str) -> Vec<String> {
        self.types
            .get(object_type)
            .map(|subtypes| subtypes.keys().cloned().collect())
            .unwrap_or_default()
    }
}

/// `filter_default_metadata()` without its `wp_installing()` check (the use case makes it).
///
/// In WordPress's order: the key's defaults are gathered from every subtype first. With none, the value passes
/// through (`None`) and `subtype` is never called, so `get_object_subtype()` and its filter run only when a default is
/// registered. A `""`-subtype default wins without a subtype lookup; otherwise the object's subtype picks one.
/// `array_key_exists( 'default', $args )` decides what counts, so a `null` default is a default. Single: the default;
/// otherwise a one-element array.
///
/// WordPress compares the stored key with `===`, and PHP stores a key like `"10"` as the int `10`, so such a key never
/// finds its default.
pub fn default_for<E>(
    keys: &dyn MetaKeyIndex,
    meta_type: &MetaType,
    key: &MetaKey,
    single: bool,
    subtype: impl FnOnce() -> Result<String, E>,
) -> Result<Option<PhpValue>, E> {
    if is_integer_key(key.as_bytes()) {
        return Ok(None);
    }
    let defaults: Vec<(String, PhpValue)> = keys
        .subtypes(meta_type.name())
        .into_iter()
        .filter_map(|sub| {
            let default = keys.find(meta_type.name(), &sub, key)?.default()?.clone();
            Some((sub, default))
        })
        .collect();
    if defaults.is_empty() {
        return Ok(None);
    }
    let pick = |sub: &str| {
        defaults
            .iter()
            .find(|(s, _)| s == sub)
            .map(|(_, d)| d.clone())
    };
    let default = match pick("") {
        Some(default) => default,
        None => match pick(&subtype()?) {
            Some(default) => default,
            None => return Ok(None),
        },
    };
    Ok(Some(if single {
        default
    } else {
        PhpValue::Array(vec![(PhpKey::Int(0), default)])
    }))
}

/// `unregister_meta_key()`'s `remove_filter()` calls (at the default priority 10) for an entry
/// [`MetaKeyRegistry::remove`] returned: the sanitize, then the auth callback, each when set and callable.
pub fn filters_to_remove(
    object_type: &str,
    removed: &RegisteredMetaKey,
    callables: &dyn Callables,
) -> Vec<(Vec<u8>, PhpValue)> {
    let names = MetaHookNames::for_object_type(object_type);
    let subtype =
        (!removed.subtype.is_empty() && removed.subtype != "0").then_some(removed.subtype.as_str());
    let mut out = Vec::new();
    if let Some(cb) = isset(&removed.args, "sanitize_callback").filter(|c| callables.is_callable(c))
    {
        out.push((names.sanitize(&removed.key, subtype), cb.clone()));
    }
    if let Some(cb) = isset(&removed.args, "auth_callback").filter(|c| callables.is_callable(c)) {
        out.push((names.auth(&removed.key, subtype), cb.clone()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn never() -> Result<String, ()> {
        panic!("asked only when a default is registered and no \"\" default exists");
    }

    #[test]
    fn integer_like_key_never_finds_its_default() {
        let mut registry = MetaKeyRegistry::new();
        let key = MetaKey::from("10");
        registry.insert(RegisteredMetaKey {
            object_type: "post".into(),
            subtype: String::new(),
            key: key.clone(),
            args: vec![(PhpKey::Str(b"default".to_vec()), PhpValue::Int(1))],
        });
        let post = MetaType::new("post").unwrap();
        assert_eq!(default_for(&registry, &post, &key, true, never), Ok(None));
    }
}
