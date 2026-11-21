//! The default route table store: route data in the worker's memory.
//!
//! A worker keeps the data of its recently used tables, so a table whose fingerprint comes back (a rule flush undone)
//! is not rebuilt. It keeps at most a fixed number of them, dropping the least recently used, so a site whose rules
//! change often does not grow the worker without end.

use std::cell::RefCell;
use std::collections::HashMap;

use speedyspec_routing::{Fingerprint, RouteData, RouteTableStore, StoreError};

#[derive(Debug)]
pub struct MemoryRouteTableStore {
    entries: RefCell<HashMap<Fingerprint, RouteData>>,
    /// Fingerprints held, most recently used first.
    order: RefCell<Vec<Fingerprint>>,
    capacity: usize,
}

impl Default for MemoryRouteTableStore {
    fn default() -> MemoryRouteTableStore {
        MemoryRouteTableStore::with_capacity(MemoryRouteTableStore::DEFAULT_CAPACITY)
    }
}

impl MemoryRouteTableStore {
    /// How many tables [`MemoryRouteTableStore::new`] keeps.
    pub const DEFAULT_CAPACITY: usize = 16;

    pub fn new() -> MemoryRouteTableStore {
        MemoryRouteTableStore::default()
    }

    /// A store keeping up to `capacity` tables (at least one).
    pub fn with_capacity(capacity: usize) -> MemoryRouteTableStore {
        MemoryRouteTableStore {
            entries: RefCell::new(HashMap::new()),
            order: RefCell::new(Vec::new()),
            capacity: capacity.max(1),
        }
    }

    /// Marks `fingerprint` the most recently used.
    fn touch(&self, fingerprint: &Fingerprint) {
        let mut order = self.order.borrow_mut();
        order.retain(|held| held != fingerprint);
        order.insert(0, *fingerprint);
    }
}

impl RouteTableStore for MemoryRouteTableStore {
    fn get(&self, fingerprint: &Fingerprint) -> Result<Option<RouteData>, StoreError> {
        let found = self.entries.borrow().get(fingerprint).cloned();
        if found.is_some() {
            self.touch(fingerprint);
        }
        Ok(found)
    }

    fn put(&self, fingerprint: &Fingerprint, data: &RouteData) -> Result<(), StoreError> {
        self.entries.borrow_mut().insert(*fingerprint, data.clone());
        self.touch(fingerprint);
        let dropped = {
            let mut order = self.order.borrow_mut();
            let keep = order.len().min(self.capacity);
            order.split_off(keep)
        };
        let mut entries = self.entries.borrow_mut();
        for fingerprint in dropped {
            entries.remove(&fingerprint);
        }
        Ok(())
    }

    fn forget(&self, fingerprint: &Fingerprint) -> Result<(), StoreError> {
        self.entries.borrow_mut().remove(fingerprint);
        self.order.borrow_mut().retain(|held| held != fingerprint);
        Ok(())
    }
}
