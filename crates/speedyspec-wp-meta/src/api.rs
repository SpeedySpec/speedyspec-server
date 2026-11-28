//! `MetadataApi`: the metadata use cases over their ports, and the value helpers they share.
//!
//! Template Method: one generic use case per `meta.php` function, with the per-type names (table, columns, cache group,
//! hooks) supplied by the [`MetaType`] Strategy and [`MetaHookNames`]. Observer: every filter and action goes through
//! the [`HookApi`] port. Persistence goes through the Repository per meta type ([`MetaRepositories`]); the use cases
//! never see SQL.

use std::rc::Rc;

use speedyspec_php_serialize::semantics::{is_empty, is_integer_key, to_array_key, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue, from_hook_value, maybe_serialize, to_hook_value};
use speedyspec_wp_hooks_contracts::{Count, HookApi, HookError, HookName, HookValue};
use speedyspec_wp_meta_contracts::{
    Installing, Meta, MetaId, MetaKey, MetaKeys, MetaRepositories, MetaType, Metadata,
    MetadataError, ObjectId, ObjectSubtypes, ProtectedMeta, SerializedValue, UpdateOutcome,
};
use speedyspec_wp_object_cache_contracts::ObjectCache;

/// What the metadata use cases work through. The runtime passes the concrete adapters in.
#[derive(Clone)]
pub struct MetaDeps {
    pub hooks: Rc<dyn HookApi>,
    /// The request's current object cache.
    pub cache: Rc<dyn ObjectCache>,
    /// `_get_meta_table()` and the Repository per meta type.
    pub repositories: Rc<dyn MetaRepositories>,
    /// `$wp_meta_keys`, read through the [`MetaKeys`] port (with the registered subtypes
    /// `filter_default_metadata()` walks).
    pub keys: Rc<dyn MetaKeys>,
    /// `get_object_subtype()`'s switch, before its filter.
    pub subtypes: Rc<dyn ObjectSubtypes>,
    /// `is_protected_meta()`'s unfiltered rule ([`UnfilteredProtection`](crate::UnfilteredProtection) or a PHP
    /// twin of it). The use cases apply the `is_protected_meta` filter once over it, so never bind a filtered
    /// port ([`FilteredProtection`](crate::FilteredProtection)) here.
    pub protection: Rc<dyn ProtectedMeta>,
    /// `wp_installing()`.
    pub installing: Rc<dyn Installing>,
}

/// WordPress's metadata API (`meta.php`), one method per function. It implements the
/// [`Metadata`](speedyspec_wp_meta_contracts::Metadata) contract and handles every metadata message
/// ([`register_meta_handlers`](crate::register_meta_handlers)).
pub struct MetadataApi {
    pub(crate) d: MetaDeps,
}

impl MetadataApi {
    pub fn new(deps: MetaDeps) -> Rc<MetadataApi> {
        Rc::new(MetadataApi { d: deps })
    }

    pub fn deps(&self) -> &MetaDeps {
        &self.d
    }

    /// `apply_filters()`, through the fast path when nothing listens.
    pub(crate) fn filter(
        &self,
        hook: HookName,
        value: HookValue,
        args: impl FnOnce() -> Vec<HookValue>,
    ) -> Result<HookValue, MetadataError> {
        filter(&*self.d.hooks, hook, value, args)
    }

    /// `do_action()`, through the fast path when nothing listens.
    pub(crate) fn action(
        &self,
        hook: impl Into<HookName>,
        args: impl FnOnce() -> Vec<HookValue>,
    ) -> Result<(), MetadataError> {
        let name = hook.into();
        if self.d.hooks.skip_if_empty(&name, Count::Actions)? {
            return Ok(());
        }
        Ok(self.d.hooks.do_action(&name, args())?)
    }

    /// `do_action()` for an old-style hook a type may not have.
    pub(crate) fn legacy_action(
        &self,
        hook: Option<impl Into<HookName>>,
        args: impl FnOnce() -> Vec<HookValue>,
    ) -> Result<(), MetadataError> {
        match hook {
            Some(hook) => self.action(hook, args),
            None => Ok(()),
        }
    }

    /// `is_protected_meta()`: the rule, then the filter, once. A callback's failure is [`MetadataError::Hook`].
    pub fn is_protected_meta(&self, key: &MetaKey, meta_type: &str) -> Result<bool, MetadataError> {
        crate::is_protected_meta(&*self.d.hooks, &*self.d.protection, key, meta_type)
    }

    /// `is_protected_meta()` as WordPress returns it: the filter's answer, which a callback may make any value.
    pub fn protected_meta_answer(
        &self,
        key: &MetaKey,
        meta_type: &str,
    ) -> Result<PhpValue, MetadataError> {
        crate::protected_meta_answer(&*self.d.hooks, &*self.d.protection, key, meta_type)
    }

    /// `register_meta()`'s second half ([`resolve_registration`](crate::resolve_registration)), asking
    /// `is_protected_meta()` (filtered once) when no `auth_callback` was given. A failing `is_protected_meta`
    /// callback is [`MetadataError::Hook`], payload intact, and nothing is registered.
    pub fn resolve_registration(
        &self,
        object_type: &str,
        key: &MetaKey,
        prepared: crate::PreparedArgs,
        revisions: &dyn speedyspec_wp_meta_contracts::RevisionSupport,
        schema: &dyn speedyspec_wp_meta_contracts::DefaultSchemaCheck,
        callables: &dyn speedyspec_wp_meta_contracts::Callables,
    ) -> Result<crate::Registration, MetadataError> {
        let protection =
            crate::FilteredProtection::new(self.d.hooks.clone(), self.d.protection.clone());
        protection.checked(|protected| {
            crate::resolve_registration(
                object_type,
                key,
                prepared,
                &crate::RegisterPorts {
                    revisions,
                    schema,
                    callables,
                    protected,
                },
            )
        })
    }

    /// `get_object_subtype()`: the port's answer through the `get_object_subtype_{type}` filter.
    pub(crate) fn subtype(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
    ) -> Result<String, MetadataError> {
        crate::object_subtype(&*self.d.hooks, &*self.d.subtypes, meta_type, object)
    }
}

/// `apply_filters()` through the fast path when nothing listens.
pub(crate) fn filter(
    hooks: &dyn HookApi,
    hook: HookName,
    value: HookValue,
    args: impl FnOnce() -> Vec<HookValue>,
) -> Result<HookValue, MetadataError> {
    if hooks.skip_if_empty(&hook, Count::Filters)? {
        return Ok(value);
    }
    Ok(hooks.apply_filters(&hook, value, args())?)
}

/// `{type}_meta`, the cache group.
pub(crate) fn group(meta_type: &MetaType) -> HookValue {
    // MetaType::cache_group(), sized once: this runs on every cached read.
    let name = meta_type.name();
    let mut group = String::with_capacity(name.len() + "_meta".len());
    group.push_str(name);
    group.push_str("_meta");
    HookValue::Str(group)
}

/// A PHP value as the hooks carry it.
pub(crate) fn hv(value: &PhpValue) -> HookValue {
    to_hook_value(value)
}

/// A hook's value as a PHP value. A foreign opaque value (a live object with no serialized form) cannot be one.
pub(crate) fn pv(value: &HookValue) -> Result<PhpValue, MetadataError> {
    from_hook_value(value).map_err(|e| {
        MetadataError::Hook(HookError::message(format!(
            "a metadata hook returned a value the metadata API cannot use: {e}"
        )))
    })
}

/// A byte string as the hooks carry it: text when it is UTF-8.
pub(crate) fn bytes_value(bytes: &[u8]) -> HookValue {
    match std::str::from_utf8(bytes) {
        Ok(s) => HookValue::Str(s.to_owned()),
        Err(_) => HookValue::Bytes(bytes.to_vec()),
    }
}

/// A hook name built from bytes (a key may be any bytes).
pub(crate) fn byte_name(bytes: Vec<u8>) -> HookName {
    match String::from_utf8(bytes) {
        Ok(s) => HookName::from(HookValue::Str(s)),
        Err(e) => HookName::from(HookValue::Bytes(e.into_bytes())),
    }
}

/// The key as a hook argument.
pub(crate) fn key_value(key: &MetaKey) -> HookValue {
    bytes_value(key.as_bytes())
}

/// An object id as PHP's int.
pub(crate) fn int(id: u64) -> HookValue {
    HookValue::Int(i64::try_from(id).unwrap_or(i64::MAX))
}

/// An id as `$wpdb->get_col()` returns it: a decimal string.
pub(crate) fn col(id: u64) -> HookValue {
    HookValue::Str(id.to_string())
}

/// `! $meta_key`: the empty string and `"0"` are falsy.
pub(crate) fn falsy_key(key: &MetaKey) -> bool {
    matches!(key.as_bytes(), b"" | b"0")
}

/// `$array[ $meta_key ]`'s key, as PHP normalizes it (`"10"` is the int `10`).
pub(crate) fn php_key(key: &MetaKey) -> PhpKey {
    let bytes = key.as_bytes();
    if !is_integer_key(bytes) {
        // to_array_key()'s answer for any other string, without the copy it would make first.
        return PhpKey::Str(bytes.to_vec());
    }
    to_array_key(&PhpValue::Str(bytes.to_vec())).expect("a string is always a key")
}

/// `$array[ $key ]` on a PHP array.
pub(crate) fn lookup<'a>(items: &'a [(PhpKey, PhpValue)], key: &PhpKey) -> Option<&'a PhpValue> {
    items.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// PHP's truthiness.
pub(crate) fn truthy(value: &PhpValue) -> bool {
    !is_empty(value)
}

/// What `$wpdb` stores for `maybe_serialize( $value )`: `NULL` for null, the string PHP prints otherwise.
pub(crate) fn stored(serialized: &PhpValue) -> Option<SerializedValue> {
    match serialized {
        PhpValue::Null => None,
        other => Some(SerializedValue::new(to_php_string(other))),
    }
}

/// `maybe_serialize( $value )` and what `$wpdb` stores for it.
pub(crate) fn serialize_for_db(value: &PhpValue) -> (PhpValue, Option<SerializedValue>) {
    let serialized = maybe_serialize(value);
    let bytes = stored(&serialized);
    (serialized, bytes)
}

/// A stored `meta_value` as `$wpdb` returns it: a string, or null for `NULL`.
pub(crate) fn column(value: &Option<SerializedValue>) -> PhpValue {
    match value {
        Some(v) => PhpValue::Str(v.as_bytes().to_vec()),
        None => PhpValue::Null,
    }
}

/// PHP's `===`. Two objects are identical only as one instance, which values read from storage never are.
pub(crate) fn identical(a: &PhpValue, b: &PhpValue) -> bool {
    use PhpValue::*;
    match (a, b) {
        (Null, Null) => true,
        (Bool(x), Bool(y)) => x == y,
        (Int(x), Int(y)) => x == y,
        (Float(x), Float(y)) => x == y,
        (Str(x), Str(y)) => x == y,
        (Array(x), Array(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|((kx, vx), (ky, vy))| kx == ky && identical(vx, vy))
        }
        (
            Enum {
                class: c1,
                case: k1,
            },
            Enum {
                class: c2,
                case: k2,
            },
        ) => c1 == c2 && k1 == k2,
        _ => false,
    }
}

/// `$value[0]`: index 0, `null` when it is missing (PHP warns "Undefined array key 0").
pub(crate) fn first(items: &[(PhpKey, PhpValue)]) -> PhpValue {
    lookup(items, &PhpKey::Int(0))
        .cloned()
        .unwrap_or(PhpValue::Null)
}

/// The [`Metadata`] contract over the use cases. Where WordPress returns a short-circuit filter's answer as it is,
/// the contract's narrower types keep what they can carry (see [`AddOutcome::mid`](crate::AddOutcome::mid)); the
/// inherent methods and the messages keep the whole answer.
impl Metadata for MetadataApi {
    fn add(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: &MetaKey,
        value: PhpValue,
        unique: bool,
    ) -> Result<Option<MetaId>, MetadataError> {
        Ok(self
            .add_metadata(meta_type, object, key, value, unique)?
            .mid())
    }

    fn update(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: &MetaKey,
        value: PhpValue,
        prev: Option<PhpValue>,
    ) -> Result<UpdateOutcome, MetadataError> {
        self.update_metadata(meta_type, object, key, value, prev)
    }

    fn delete(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: &MetaKey,
        value: Option<PhpValue>,
        delete_all: bool,
    ) -> Result<bool, MetadataError> {
        self.delete_metadata(meta_type, object, key, value, delete_all)
    }

    fn get(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: Option<&MetaKey>,
        single: bool,
    ) -> Result<PhpValue, MetadataError> {
        self.get_metadata(meta_type, object, key, single)
    }

    fn get_raw(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: Option<&MetaKey>,
        single: bool,
    ) -> Result<Option<PhpValue>, MetadataError> {
        self.get_metadata_raw(meta_type, object, key, single)
    }

    fn get_default(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: &MetaKey,
        single: bool,
    ) -> Result<PhpValue, MetadataError> {
        self.get_metadata_default(meta_type, object, key, single)
    }

    fn exists(
        &self,
        meta_type: &MetaType,
        object: ObjectId,
        key: &MetaKey,
    ) -> Result<bool, MetadataError> {
        self.metadata_exists(meta_type, object, key)
    }

    fn get_by_mid(&self, meta_type: &MetaType, mid: MetaId) -> Result<Option<Meta>, MetadataError> {
        Ok(match self.get_metadata_by_mid(meta_type, mid)? {
            crate::ByMidOutcome::Found(meta) => Some(meta),
            _ => None,
        })
    }

    fn update_by_mid(
        &self,
        meta_type: &MetaType,
        mid: MetaId,
        value: PhpValue,
        key: Option<&MetaKey>,
    ) -> Result<bool, MetadataError> {
        self.update_metadata_by_mid(meta_type, mid, value, key)
    }

    fn delete_by_mid(&self, meta_type: &MetaType, mid: MetaId) -> Result<bool, MetadataError> {
        self.delete_metadata_by_mid(meta_type, mid)
    }

    fn update_cache(
        &self,
        meta_type: &MetaType,
        objects: &[ObjectId],
    ) -> Result<Option<PhpValue>, MetadataError> {
        self.update_meta_cache(meta_type, objects)
    }
}
