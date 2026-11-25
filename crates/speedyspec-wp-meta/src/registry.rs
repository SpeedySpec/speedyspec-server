//! `$wp_meta_keys` (Registry): the keys `register_meta()` stored, and what `unregister_meta_key()`,
//! `registered_meta_key_exists()`, `get_registered_meta_keys()` and `filter_default_metadata()` read from it.
//! Ported from WordPress 7.1.5's `wp-includes/meta.php`.

use indexmap::IndexMap;
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_meta_contracts::{Callables, MetaKey, MetaType, ObjectId, ObjectSubtypes};

use crate::hook_names::MetaHookNames;
use crate::php::{is_integer_key, isset};
use crate::register::RegisteredMetaKey;

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

    /// `get_registered_meta_keys()`: empty for an unknown type or subtype, in insertion order.
    pub fn keys(&self, object_type: &str, subtype: &str) -> Vec<&RegisteredMetaKey> {
        self.level(object_type, subtype)
            .map(|keys| keys.values().collect())
            .unwrap_or_default()
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

    /// filter_default_metadata() without its wp_installing() check (the phase-3 use case makes it).
    ///
    /// `None` means the value passes through unchanged. Single: the default; otherwise a one-element array.
    /// `ObjectSubtypes` is asked only when no `""`-subtype default exists. WordPress compares the stored key with
    /// `===`, and PHP stores a key like `"10"` as the int `10`, so such a key never finds its default.
    pub fn default_for(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: &MetaKey,
        single: bool,
        subtypes: &dyn ObjectSubtypes,
    ) -> Option<PhpValue> {
        if is_integer_key(key.as_bytes()) {
            return None;
        }
        let defaults: Vec<(&str, &PhpValue)> = self
            .types
            .get(meta_type.name())?
            .iter()
            .filter_map(|(subtype, keys)| Some((subtype.as_str(), keys.get(key)?.default()?)))
            .collect();
        if defaults.is_empty() {
            return None;
        }
        let find = |wanted: &str| defaults.iter().find(|(s, _)| *s == wanted).map(|(_, d)| *d);
        let default = match find("") {
            Some(default) => default,
            None => find(&subtypes.subtype_of(meta_type, object))?,
        };
        Some(if single {
            default.clone()
        } else {
            PhpValue::Array(vec![(PhpKey::Int(0), default.clone())])
        })
    }
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

    struct NoSubtype;
    impl ObjectSubtypes for NoSubtype {
        fn subtype_of(&self, _: &MetaType, _: ObjectId) -> String {
            panic!("asked only without a \"\" default");
        }
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
        assert_eq!(
            registry.default_for(&post, ObjectId(1), &key, true, &NoSubtype),
            None
        );
    }
}
