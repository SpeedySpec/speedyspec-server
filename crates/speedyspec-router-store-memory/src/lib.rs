//! The default route table store: route data in the worker's memory.
//!
//! A worker keeps each table's data for as long as it lives, so a table whose fingerprint comes back (a rule flush
//! undone) is not rebuilt.

use std::cell::RefCell;
use std::collections::HashMap;

use speedyspec_routing::{Fingerprint, RouteData, RouteTableStore, StoreError};

#[derive(Debug, Default)]
pub struct MemoryRouteTableStore {
    entries: RefCell<HashMap<Fingerprint, RouteData>>,
}

impl MemoryRouteTableStore {
    pub fn new() -> MemoryRouteTableStore {
        MemoryRouteTableStore::default()
    }
}

impl RouteTableStore for MemoryRouteTableStore {
    fn get(&self, fingerprint: &Fingerprint) -> Result<Option<RouteData>, StoreError> {
        Ok(self.entries.borrow().get(fingerprint).cloned())
    }

    fn put(&self, fingerprint: &Fingerprint, data: &RouteData) -> Result<(), StoreError> {
        self.entries.borrow_mut().insert(*fingerprint, data.clone());
        Ok(())
    }

    fn forget(&self, fingerprint: &Fingerprint) -> Result<(), StoreError> {
        self.entries.borrow_mut().remove(fingerprint);
        Ok(())
    }
}
