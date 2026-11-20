//! `WP_Object_Cache`: the first level (values held as they are), Decorator over an optional second level.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};
use speedyspec_cache::CachingStore;
use speedyspec_hooks::{Key, Value};

use crate::keys::{array_key, gettype, key_text};
use crate::{ObjectCacheError, ObjectCachePorts, numeric};

/// A first-level entry: a PHP-shaped value, or second-level bytes not decoded yet.
#[derive(Debug, Clone, PartialEq)]
pub enum CacheValue {
    Php(Value),
    Bytes(Vec<u8>),
}

type Groups = IndexMap<Key, IndexMap<Key, CacheValue>>;

/// WordPress's object cache for one `WP_Object_Cache` instance.
pub struct ObjectCache {
    pub(crate) ports: ObjectCachePorts,
    pub(crate) persistent: Option<Rc<dyn CachingStore>>,
    pub(crate) cache: RefCell<Groups>,
    hits: Cell<i64>,
    misses: Cell<i64>,
    global_groups: RefCell<IndexMap<Key, ()>>,
    pub(crate) non_persistent: RefCell<IndexSet<Key>>,
    blog_prefix: RefCell<String>,
    multisite: Cell<bool>,
    /// The second-level failure kinds already logged by this cache (one cache per request).
    pub(crate) failures_logged: Cell<u8>,
}

/// PHP's `empty()`.
pub(crate) fn empty_group(v: &Value) -> bool {
    empty(v)
}

/// PHP's `empty()`.
fn empty(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(false) | Value::Int(0) => true,
        Value::Float(f) => *f == 0.0,
        Value::Str(s) => s.is_empty() || s == "0",
        Value::Bytes(b) => b.is_empty() || b == b"0",
        Value::Array(a) => a.is_empty(),
        _ => false,
    }
}

fn php_trim_empty(s: &str) -> bool {
    s.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
        .is_empty()
}

impl ObjectCache {
    /// Reads the site once: on multisite the blog prefix is `"<id>:"`.
    pub fn new(ports: ObjectCachePorts, persistent: Option<Rc<dyn CachingStore>>) -> ObjectCache {
        let multisite = ports.site.is_multisite();
        let prefix = if multisite {
            format!("{}:", ports.site.current_blog_id())
        } else {
            String::new()
        };
        ObjectCache {
            ports,
            persistent,
            cache: RefCell::new(IndexMap::new()),
            hits: Cell::new(0),
            misses: Cell::new(0),
            global_groups: RefCell::new(IndexMap::new()),
            non_persistent: RefCell::new(IndexSet::new()),
            blog_prefix: RefCell::new(prefix),
            multisite: Cell::new(multisite),
            failures_logged: Cell::new(0),
        }
    }

    /// The same cache with `store` as its second level.
    pub fn with_persistent(mut self, store: Rc<dyn CachingStore>) -> ObjectCache {
        self.persistent = Some(store);
        self
    }

    /// Whether a second level shares values across requests and processes, as a persistent drop-in does.
    pub fn is_persistent(&self) -> bool {
        self.persistent.is_some()
    }

    /// `is_valid_key`: the key as an array key, or a notice naming the method that checked it.
    fn valid_key(&self, key: &Value, method: &str) -> Result<Option<Key>, ObjectCacheError> {
        let ok = match key {
            Value::Int(_) => true,
            Value::Str(s) => !php_trim_empty(s),
            Value::Bytes(b) => !String::from_utf8_lossy(b)
                .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                .is_empty(),
            _ => false,
        };
        if ok {
            return array_key(key).map(Some);
        }
        let message = if matches!(key, Value::Str(_) | Value::Bytes(_)) {
            "Cache key must not be an empty string.".to_string()
        } else {
            format!(
                "Cache key must be an integer or a non-empty string, {} given.",
                gettype(key)
            )
        };
        self.ports
            .notices
            .doing_it_wrong(&format!("WP_Object_Cache::{method}"), &message, "6.1.0");
        Ok(None)
    }

    /// The group, `default` when empty.
    fn group(&self, group: &Value) -> Result<Key, ObjectCacheError> {
        if empty(group) {
            Ok(Key::Str("default".into()))
        } else {
            array_key(group)
        }
    }

    pub(crate) fn prefixed(&self, key: &Key, group: &Key) -> Key {
        self.id(key, group)
    }

    /// The entry's id: on multisite, outside the global groups, the blog prefix and the key.
    fn id(&self, key: &Key, group: &Key) -> Key {
        if self.multisite.get() && !self.global_groups.borrow().contains_key(group) {
            Key::Str(format!("{}{}", self.blog_prefix.borrow(), key_text(key)))
        } else {
            key.clone()
        }
    }

    /// Whether `key` is in the first level, counting no hit or miss and raising no notice (an invalid key is
    /// absent). For callers that must know what a read would do without doing it.
    pub fn exists(&self, key: &Value, group: &Value) -> bool {
        self.quiet_id(key, group)
            .is_some_and(|(id, g)| self.has_entry(&id, &g))
    }

    /// The first level's value for `key`, as [`ObjectCache::exists`] looks: no count, no notice. Bytes from the
    /// second level not yet read are `None`.
    pub fn peek(&self, key: &Value, group: &Value) -> Option<Value> {
        let (id, g) = self.quiet_id(key, group)?;
        match self.cache.borrow().get(&g)?.get(&id)? {
            CacheValue::Php(v) => Some(v.clone()),
            CacheValue::Bytes(_) => None,
        }
    }

    fn quiet_id(&self, key: &Value, group: &Value) -> Option<(Key, Key)> {
        let valid = match key {
            Value::Int(_) => true,
            Value::Str(s) => !php_trim_empty(s),
            _ => false,
        };
        if !valid {
            return None;
        }
        let key = array_key(key).ok()?;
        let group = self.group(group).ok()?;
        Some((self.id(&key, &group), group))
    }

    pub(crate) fn has_entry(&self, id: &Key, group: &Key) -> bool {
        self.cache
            .borrow()
            .get(group)
            .is_some_and(|g| g.contains_key(id))
    }

    pub(crate) fn store(&self, id: Key, group: Key, value: CacheValue) {
        self.cache
            .borrow_mut()
            .entry(group)
            .or_default()
            .insert(id, value);
    }

    pub(crate) fn decode_entry(&self, id: &Key, group: &Key) -> Option<Value> {
        let entry = self.cache.borrow().get(group)?.get(id).cloned()?;
        match entry {
            CacheValue::Php(v) => Some(v),
            CacheValue::Bytes(bytes) => match self.ports.codec.decode(&bytes) {
                Ok(v) => {
                    self.store(id.clone(), group.clone(), CacheValue::Php(v.clone()));
                    Some(v)
                }
                Err(e) => {
                    self.ports.notices.persistence_miss(
                        &key_text(group),
                        &key_text(id),
                        &format!("{e:?}"),
                    );
                    if let Some(g) = self.cache.borrow_mut().get_mut(group) {
                        g.shift_remove(id);
                    }
                    // Bytes come only from the second level: left there, the bad row would block `add` until it
                    // expires.
                    self.forget_second(id, group);
                    None
                }
            },
        }
    }

    /// Resolves the key and group for `method`; `None` when the key was refused.
    fn target(
        &self,
        key: &Value,
        group: &Value,
        method: &str,
    ) -> Result<Option<(Key, Key)>, ObjectCacheError> {
        let Some(key) = self.valid_key(key, method)? else {
            return Ok(None);
        };
        let group = self.group(group)?;
        Ok(Some((self.id(&key, &group), group)))
    }

    pub fn add(
        &self,
        key: &Value,
        data: Value,
        group: &Value,
        expire: i64,
    ) -> Result<bool, ObjectCacheError> {
        if self.ports.suspension.suspended() {
            return Ok(false);
        }
        let Some((id, g)) = self.target(key, group, "add")? else {
            return Ok(false);
        };
        if self.has_entry(&id, &g) {
            return Ok(false);
        }
        // With a second level, its own atomic `add` decides (another worker may hold the key) and writes it.
        match self.second_add(&id, &g, &data, expire) {
            None => self.write(id, g, data, expire, false),
            Some(false) => return Ok(false),
            Some(true) => self.store(id, g, CacheValue::Php(data)),
        }
        Ok(true)
    }

    pub fn add_multiple(
        &self,
        data: Vec<(Key, Value)>,
        group: &Value,
        expire: i64,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError> {
        let mut out = Vec::with_capacity(data.len());
        for (k, v) in data {
            let r = self.add(&key_value(&k), v, group, expire)?;
            out.push((k, r));
        }
        Ok(out)
    }

    pub fn replace(
        &self,
        key: &Value,
        data: Value,
        group: &Value,
        expire: i64,
    ) -> Result<bool, ObjectCacheError> {
        let Some((id, g)) = self.target(key, group, "replace")? else {
            return Ok(false);
        };
        if !self.has_entry(&id, &g) && !self.second_has(&id, &g) {
            return Ok(false);
        }
        self.write(id, g, data, expire, true);
        Ok(true)
    }

    pub fn set(
        &self,
        key: &Value,
        data: Value,
        group: &Value,
        expire: i64,
    ) -> Result<bool, ObjectCacheError> {
        let Some((id, g)) = self.target(key, group, "set")? else {
            return Ok(false);
        };
        self.write(id, g, data, expire, true);
        Ok(true)
    }

    pub fn set_multiple(
        &self,
        data: Vec<(Key, Value)>,
        group: &Value,
        expire: i64,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError> {
        let mut out = Vec::with_capacity(data.len());
        for (k, v) in data {
            let r = self.set(&key_value(&k), v, group, expire)?;
            out.push((k, r));
        }
        Ok(out)
    }

    /// `None` is a miss (PHP's `false` with `$found = false`).
    pub fn get(
        &self,
        key: &Value,
        group: &Value,
        force: bool,
    ) -> Result<Option<Value>, ObjectCacheError> {
        let Some((id, g)) = self.target(key, group, "get")? else {
            return Ok(None);
        };
        Ok(self.read(&id, &g, force))
    }

    /// `get`, with the value lent to `f` instead of copied: a hit or miss is counted as `get` counts it. `f` must
    /// not call back into this cache.
    pub fn get_with<R>(
        &self,
        key: &Value,
        group: &Value,
        force: bool,
        f: impl FnOnce(Option<&Value>) -> R,
    ) -> Result<R, ObjectCacheError> {
        let Some((id, g)) = self.target(key, group, "get")? else {
            return Ok(f(None));
        };
        if !(force && self.persists(&g)) {
            let cache = self.cache.borrow();
            if let Some(CacheValue::Php(v)) = cache.get(&g).and_then(|entries| entries.get(&id)) {
                self.hits.set(self.hits.get() + 1);
                return Ok(f(Some(v)));
            }
        }
        let value = self.read(&id, &g, force);
        Ok(f(value.as_ref()))
    }

    pub(crate) fn read(&self, id: &Key, g: &Key, force: bool) -> Option<Value> {
        if !(force && self.persists(g))
            && self.has_entry(id, g)
            && let Some(v) = self.decode_entry(id, g)
        {
            self.hits.set(self.hits.get() + 1);
            return Some(v);
        }
        if let Some(v) = self.second_get(id, g) {
            self.hits.set(self.hits.get() + 1);
            return Some(v);
        }
        self.misses.set(self.misses.get() + 1);
        None
    }

    /// Each key's value, `false` for a miss.
    pub fn get_multiple(
        &self,
        keys: &[Value],
        group: &Value,
        force: bool,
    ) -> Result<Vec<(Key, Value)>, ObjectCacheError> {
        let mut out = Vec::with_capacity(keys.len());
        self.prefetch(keys, group, force)?;
        for key in keys {
            let k = array_key(key).unwrap_or(Key::Str(String::new()));
            let v = self.get(key, group, false)?.unwrap_or(Value::Bool(false));
            match out
                .iter_mut()
                .find(|(existing, _): &&mut (Key, Value)| *existing == k)
            {
                Some(slot) => slot.1 = v,
                None => out.push((k, v)),
            }
        }
        Ok(out)
    }

    pub fn delete(&self, key: &Value, group: &Value) -> Result<bool, ObjectCacheError> {
        let Some((id, g)) = self.target(key, group, "delete")? else {
            return Ok(false);
        };
        let first = self
            .cache
            .borrow_mut()
            .get_mut(&g)
            .and_then(|e| e.shift_remove(&id))
            .is_some();
        let second = self.second_delete(&id, &g);
        Ok(first || second)
    }

    pub fn delete_multiple(
        &self,
        keys: &[Value],
        group: &Value,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError> {
        let mut out = Vec::with_capacity(keys.len());
        for key in keys {
            let k = array_key(key).unwrap_or(Key::Str(String::new()));
            let r = self.delete(key, group)?;
            match out
                .iter_mut()
                .find(|(existing, _): &&mut (Key, bool)| *existing == k)
            {
                Some(slot) => slot.1 = r,
                None => out.push((k, r)),
            }
        }
        Ok(out)
    }

    fn adjust(
        &self,
        key: &Value,
        offset: i64,
        group: &Value,
        method: &str,
    ) -> Result<Option<Value>, ObjectCacheError> {
        let Some((id, g)) = self.target(key, group, method)? else {
            return Ok(None);
        };
        let Some(current) = self.read_quiet(&id, &g) else {
            return Ok(None);
        };
        let next = numeric::add(&current, offset);
        self.write(id, g, next.clone(), 0, true);
        Ok(Some(next))
    }

    /// A read for `incr`/`decr`, which WordPress does not count as a hit or a miss.
    fn read_quiet(&self, id: &Key, g: &Key) -> Option<Value> {
        let (hits, misses) = (self.hits.get(), self.misses.get());
        let v = self.read(id, g, false);
        self.hits.set(hits);
        self.misses.set(misses);
        v
    }

    pub fn incr(
        &self,
        key: &Value,
        offset: i64,
        group: &Value,
    ) -> Result<Option<Value>, ObjectCacheError> {
        self.adjust(key, offset, group, "incr")
    }

    pub fn decr(
        &self,
        key: &Value,
        offset: i64,
        group: &Value,
    ) -> Result<Option<Value>, ObjectCacheError> {
        self.adjust(key, offset.checked_neg().unwrap_or(i64::MAX), group, "decr")
    }

    pub fn flush(&self) -> bool {
        self.cache.borrow_mut().clear();
        self.second_flush();
        true
    }

    /// `wp_cache_flush_runtime()`: the first level only.
    pub fn flush_runtime(&self) -> bool {
        self.cache.borrow_mut().clear();
        true
    }

    /// `flush_group`: the group as given, without the `default` rule.
    pub fn flush_group(&self, group: &Value) -> Result<bool, ObjectCacheError> {
        let g = array_key(group)?;
        self.cache.borrow_mut().shift_remove(&g);
        self.second_flush_group(&g);
        Ok(true)
    }

    pub fn add_global_groups(&self, groups: &[Value]) {
        let mut global = self.global_groups.borrow_mut();
        for g in groups {
            if let Ok(k) = array_key(g) {
                global.insert(k, ());
            }
        }
    }

    pub fn add_non_persistent_groups(&self, groups: &[Value]) {
        let mut np = self.non_persistent.borrow_mut();
        for g in groups {
            if let Ok(k) = array_key(g) {
                np.insert(k);
            }
        }
    }

    pub fn switch_to_blog(&self, blog_id: i64) {
        *self.blog_prefix.borrow_mut() = if self.multisite.get() {
            format!("{blog_id}:")
        } else {
            String::new()
        };
    }

    /// `reset()` (deprecated since 3.5.0): every group but the global ones is emptied.
    pub fn reset(&self) {
        self.ports.notices.deprecated_function(
            "reset",
            "3.5.0",
            "WP_Object_Cache::switch_to_blog()",
        );
        let global = self.global_groups.borrow().clone();
        self.cache
            .borrow_mut()
            .retain(|g, _| global.contains_key(g));
    }

    /// `wp_cache_supports()`.
    pub fn supports(feature: &str) -> bool {
        matches!(
            feature,
            "add_multiple"
                | "set_multiple"
                | "get_multiple"
                | "delete_multiple"
                | "flush_runtime"
                | "flush_group"
        )
    }

    pub fn hits(&self) -> i64 {
        self.hits.get()
    }
    pub fn misses(&self) -> i64 {
        self.misses.get()
    }
    pub fn set_hits(&self, n: i64) {
        self.hits.set(n);
    }
    pub fn set_misses(&self, n: i64) {
        self.misses.set(n);
    }
    pub fn global_groups(&self) -> Vec<Key> {
        self.global_groups.borrow().keys().cloned().collect()
    }
    pub fn blog_prefix(&self) -> String {
        self.blog_prefix.borrow().clone()
    }
    pub fn set_blog_prefix(&self, prefix: String) {
        *self.blog_prefix.borrow_mut() = prefix;
    }
    pub fn is_multisite(&self) -> bool {
        self.multisite.get()
    }
    pub fn set_multisite(&self, on: bool) {
        self.multisite.set(on);
    }

    /// The `$cache` property: every group's entries, decoded.
    pub fn runtime_entries(&self) -> Vec<(Key, Vec<(Key, Value)>)> {
        let groups: Vec<(Key, Vec<Key>)> = self
            .cache
            .borrow()
            .iter()
            .map(|(g, e)| (g.clone(), e.keys().cloned().collect()))
            .collect();
        groups
            .into_iter()
            .map(|(g, ids)| {
                let entries = ids
                    .iter()
                    .filter_map(|id| self.decode_entry(id, &g).map(|v| (id.clone(), v)))
                    .collect();
                (g, entries)
            })
            .collect()
    }

    pub fn set_runtime_entries(&self, entries: Vec<(Key, Vec<(Key, Value)>)>) {
        *self.cache.borrow_mut() = entries
            .into_iter()
            .map(|(g, e)| {
                (
                    g,
                    e.into_iter()
                        .map(|(k, v)| (k, CacheValue::Php(v)))
                        .collect(),
                )
            })
            .collect();
    }
}

/// An array key back as the value a PHP caller would pass.
pub(crate) fn key_value(k: &Key) -> Value {
    match k {
        Key::Int(i) => Value::Int(*i),
        Key::Str(s) => Value::Str(s.clone()),
    }
}
