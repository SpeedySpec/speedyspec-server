//! The cache the use cases act on: the request's current `WP_Object_Cache`.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_cache::CachingStore;

use crate::{ObjectCache, ObjectCachePorts};

/// Holds the current object cache and makes others (each `new WP_Object_Cache`) sharing the ports and second level.
pub struct CurrentObjectCache {
    ports: ObjectCachePorts,
    persistent: Option<Rc<dyn CachingStore>>,
    current: RefCell<Rc<ObjectCache>>,
    made: RefCell<Vec<Rc<ObjectCache>>>,
}

impl CurrentObjectCache {
    pub fn new(
        ports: ObjectCachePorts,
        persistent: Option<Rc<dyn CachingStore>>,
    ) -> CurrentObjectCache {
        let current = Rc::new(ObjectCache::new(ports.clone(), persistent.clone()));
        CurrentObjectCache {
            ports,
            persistent,
            current: RefCell::new(current),
            made: RefCell::new(Vec::new()),
        }
    }

    /// A new cache sharing the ports and the second level.
    pub fn create(&self) -> Rc<ObjectCache> {
        let cache = Rc::new(ObjectCache::new(
            self.ports.clone(),
            self.persistent.clone(),
        ));
        self.made.borrow_mut().push(cache.clone());
        cache
    }

    pub fn current(&self) -> Rc<ObjectCache> {
        self.current.borrow().clone()
    }

    pub fn select(&self, cache: Rc<ObjectCache>) {
        *self.current.borrow_mut() = cache;
    }

    /// Drops every cache it holds, and every value in them, and starts a fresh current one.
    pub fn end_request(&self) {
        self.made.borrow_mut().clear();
        *self.current.borrow_mut() = Rc::new(ObjectCache::new(
            self.ports.clone(),
            self.persistent.clone(),
        ));
    }
}
