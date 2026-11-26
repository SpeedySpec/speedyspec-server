//! The object cache port: `wp_cache_*`, as other seams call it.

use speedyspec_hooks::{Key, Value};

use crate::ObjectCacheError;

/// WordPress's `wp_cache_*` functions, as other seams call them. An API port: a seam holds it as
/// `Rc<dyn ObjectCache>`, and `speedyspec-wp-object-cache` implements it.
///
/// `None` from a read is a miss (PHP's `false` with `$found = false`).
pub trait ObjectCache {
    /// options (`get_option()`, transients, network options), cron: `wp_cache_get()`.
    fn get(
        &self,
        key: &Value,
        group: &Value,
        force: bool,
    ) -> Result<Option<Value>, ObjectCacheError>;
    /// options (the autoloaded `alloptions` read): [`get_with`], object safe. `reader` sees the value in place
    /// (`None` on a miss), once, unless the result is `Err`. `reader` must not call back into the cache.
    ///
    /// Implementations must call `reader` on every path that returns `Ok`: [`get_with`] panics otherwise.
    fn read_with(
        &self,
        key: &Value,
        group: &Value,
        force: bool,
        reader: &mut dyn FnMut(Option<&Value>),
    ) -> Result<(), ObjectCacheError>;
    /// options (`wp_prime_option_caches()`): `wp_cache_get_multiple()`, each key's value or `false` for a miss.
    fn get_multiple(
        &self,
        keys: &[Value],
        group: &Value,
        force: bool,
    ) -> Result<Vec<(Key, Value)>, ObjectCacheError>;
    /// options: the cached value as `wp_cache_get()` would find it, with no hit or miss counted and no notice.
    fn peek(&self, key: &Value, group: &Value) -> Option<Value>;
    /// options (`add_option()`): `wp_cache_add()`, `false` when the key is already held.
    fn add(
        &self,
        key: &Value,
        data: Value,
        group: &Value,
        expire: i64,
    ) -> Result<bool, ObjectCacheError>;
    /// phase 3 (`update_meta_cache()`): `wp_cache_add_multiple()`.
    fn add_multiple(
        &self,
        data: Vec<(Key, Value)>,
        group: &Value,
        expire: i64,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError>;
    /// options (`update_option()`, transients), cron: `wp_cache_set()`.
    fn set(
        &self,
        key: &Value,
        data: Value,
        group: &Value,
        expire: i64,
    ) -> Result<bool, ObjectCacheError>;
    /// options (`wp_prime_option_caches()`): `wp_cache_set_multiple()`.
    fn set_multiple(
        &self,
        data: Vec<(Key, Value)>,
        group: &Value,
        expire: i64,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError>;
    /// options (`delete_option()`, transients), cron: `wp_cache_delete()`.
    fn delete(&self, key: &Value, group: &Value) -> Result<bool, ObjectCacheError>;
    /// options: `wp_cache_delete_multiple()`.
    fn delete_multiple(
        &self,
        keys: &[Value],
        group: &Value,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError>;
    /// options (network options, site transients): `wp_cache_add_global_groups()`.
    fn add_global_groups(&self, groups: &[Value]);
    /// options (`ObjectCachePersistence`): `wp_using_ext_object_cache()` in Rust, whether a second level shares
    /// values across requests and processes.
    fn is_persistent(&self) -> bool;
}

/// `wp_cache_get()`, with the value lent to `f` instead of copied: the generic form of
/// [`ObjectCache::read_with`], kept outside the trait so the trait stays object safe. `f` runs once, unless the
/// result is `Err`; it must not call back into the cache.
///
/// # Panics
///
/// Panics if `cache.read_with` returns `Ok` without calling its reader, which breaks the
/// [`ObjectCache::read_with`] contract.
pub fn get_with<R>(
    cache: &dyn ObjectCache,
    key: &Value,
    group: &Value,
    force: bool,
    f: impl FnOnce(Option<&Value>) -> R,
) -> Result<R, ObjectCacheError> {
    let mut f = Some(f);
    let mut out = None;
    cache.read_with(key, group, force, &mut |value| {
        if let Some(f) = f.take() {
            out = Some(f(value));
        }
    })?;
    Ok(out.expect("ObjectCache::read_with must call its reader once when it succeeds"))
}
