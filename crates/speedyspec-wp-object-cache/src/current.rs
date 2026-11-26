//! The cache the use cases act on: the request's current `WP_Object_Cache`.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_cache::CachingStore;
use speedyspec_hooks::{Key, Value};
use speedyspec_wp_object_cache_contracts::{ObjectCache, ObjectCacheError};

use crate::{ObjectCachePorts, WpObjectCache};

/// Holds the current object cache and makes others (each `new WP_Object_Cache`) sharing the ports and second level.
pub struct CurrentObjectCache {
    ports: ObjectCachePorts,
    persistent: Option<Rc<dyn CachingStore>>,
    current: RefCell<Rc<WpObjectCache>>,
    made: RefCell<Vec<Rc<WpObjectCache>>>,
}

impl CurrentObjectCache {
    pub fn new(
        ports: ObjectCachePorts,
        persistent: Option<Rc<dyn CachingStore>>,
    ) -> CurrentObjectCache {
        let current = Rc::new(WpObjectCache::new(ports.clone(), persistent.clone()));
        CurrentObjectCache {
            ports,
            persistent,
            current: RefCell::new(current),
            made: RefCell::new(Vec::new()),
        }
    }

    /// A new cache sharing the ports and the second level.
    pub fn create(&self) -> Rc<WpObjectCache> {
        let cache = Rc::new(WpObjectCache::new(
            self.ports.clone(),
            self.persistent.clone(),
        ));
        self.made.borrow_mut().push(cache.clone());
        cache
    }

    pub fn current(&self) -> Rc<WpObjectCache> {
        self.current.borrow().clone()
    }

    pub fn select(&self, cache: Rc<WpObjectCache>) {
        *self.current.borrow_mut() = cache;
    }

    /// Drops every cache it holds, and every value in them, and starts a fresh current one.
    pub fn end_request(&self) {
        self.made.borrow_mut().clear();
        *self.current.borrow_mut() = Rc::new(WpObjectCache::new(
            self.ports.clone(),
            self.persistent.clone(),
        ));
    }
}

/// The `wp_cache_*` port, on whichever cache is current at each call (as WordPress reads `$wp_object_cache` each
/// time), so a `select()` or `end_request()` between calls is seen by the next one.
impl ObjectCache for CurrentObjectCache {
    fn get(
        &self,
        key: &Value,
        group: &Value,
        force: bool,
    ) -> Result<Option<Value>, ObjectCacheError> {
        self.current().get(key, group, force)
    }

    fn read_with(
        &self,
        key: &Value,
        group: &Value,
        force: bool,
        reader: &mut dyn FnMut(Option<&Value>),
    ) -> Result<(), ObjectCacheError> {
        self.current().get_with(key, group, force, reader)
    }

    fn get_multiple(
        &self,
        keys: &[Value],
        group: &Value,
        force: bool,
    ) -> Result<Vec<(Key, Value)>, ObjectCacheError> {
        self.current().get_multiple(keys, group, force)
    }

    fn peek(&self, key: &Value, group: &Value) -> Option<Value> {
        self.current().peek(key, group)
    }

    fn add(
        &self,
        key: &Value,
        data: Value,
        group: &Value,
        expire: i64,
    ) -> Result<bool, ObjectCacheError> {
        self.current().add(key, data, group, expire)
    }

    fn add_multiple(
        &self,
        data: Vec<(Key, Value)>,
        group: &Value,
        expire: i64,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError> {
        self.current().add_multiple(data, group, expire)
    }

    fn set(
        &self,
        key: &Value,
        data: Value,
        group: &Value,
        expire: i64,
    ) -> Result<bool, ObjectCacheError> {
        self.current().set(key, data, group, expire)
    }

    fn set_multiple(
        &self,
        data: Vec<(Key, Value)>,
        group: &Value,
        expire: i64,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError> {
        self.current().set_multiple(data, group, expire)
    }

    fn delete(&self, key: &Value, group: &Value) -> Result<bool, ObjectCacheError> {
        self.current().delete(key, group)
    }

    fn delete_multiple(
        &self,
        keys: &[Value],
        group: &Value,
    ) -> Result<Vec<(Key, bool)>, ObjectCacheError> {
        self.current().delete_multiple(keys, group)
    }

    fn add_global_groups(&self, groups: &[Value]) {
        self.current().add_global_groups(groups)
    }

    fn is_persistent(&self) -> bool {
        self.current().is_persistent()
    }
}
