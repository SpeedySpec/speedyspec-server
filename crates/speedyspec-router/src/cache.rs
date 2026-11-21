use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_routing::{DispatchStrategy, Fingerprint, RouteData, RouteTableStore};

use crate::{CompileError, CompiledTable};

/// Holds the worker's recently used compiled tables and rebuilds one only when no held table has its fingerprint.
///
/// It keeps up to [`TableCache::DEFAULT_CAPACITY`] tables (or the capacity given), dropping the least recently used,
/// so app routes and WordPress's rules can share one cache without recompiling each other away. On a miss it asks
/// the store for the route data, builds and stores it on a miss, compiles it, and keeps the
/// result. Nothing passes between workers or replicas: each one compiles its own table from its own inputs.
pub struct TableCache {
    store: Rc<dyn RouteTableStore>,
    strategy: Rc<dyn DispatchStrategy>,
    /// Most recently used first.
    held: RefCell<Vec<(Fingerprint, Rc<CompiledTable>)>>,
    capacity: usize,
    store_error_logged: Cell<bool>,
}

impl TableCache {
    /// How many tables [`TableCache::new`] keeps.
    pub const DEFAULT_CAPACITY: usize = 4;

    pub fn new(store: Rc<dyn RouteTableStore>, strategy: Rc<dyn DispatchStrategy>) -> TableCache {
        TableCache::with_capacity(store, strategy, TableCache::DEFAULT_CAPACITY)
    }

    /// A cache keeping up to `capacity` tables (at least one).
    pub fn with_capacity(
        store: Rc<dyn RouteTableStore>,
        strategy: Rc<dyn DispatchStrategy>,
        capacity: usize,
    ) -> TableCache {
        TableCache {
            store,
            strategy,
            held: RefCell::new(Vec::new()),
            capacity: capacity.max(1),
            store_error_logged: Cell::new(false),
        }
    }

    fn log_store_error(&self, err: &dyn std::fmt::Display) {
        if !self.store_error_logged.replace(true) {
            eprintln!("speedyspec_router: the route table store failed, rebuilding instead: {err}");
        }
    }

    /// The table for `fingerprint`, building its data with `build` only when neither this cache nor the store has
    /// it. A store error is logged once and treated as a miss.
    pub fn table(
        &self,
        fingerprint: Fingerprint,
        build: impl FnOnce() -> Result<RouteData, CompileError>,
    ) -> Result<Rc<CompiledTable>, String> {
        {
            let mut held = self.held.borrow_mut();
            if let Some(at) = held.iter().position(|(held, _)| *held == fingerprint) {
                let entry = held.remove(at);
                let table = entry.1.clone();
                held.insert(0, entry);
                return Ok(table);
            }
        }
        // Data whose own fingerprint differs (a store with a key bug) is a miss, never another table.
        let stored = match self.store.get(&fingerprint) {
            Ok(stored) => stored.filter(|data| crate::fingerprint_of(data) == fingerprint),
            Err(err) => {
                self.log_store_error(&err);
                None
            }
        };
        let data = match stored {
            Some(data) => data,
            None => {
                let data = build().map_err(|err| err.to_string())?;
                if let Err(err) = self.store.put(&fingerprint, &data) {
                    self.log_store_error(&err);
                }
                data
            }
        };
        let table = Rc::new(CompiledTable::compile(&data, self.strategy.as_ref())?);
        let mut held = self.held.borrow_mut();
        held.insert(0, (fingerprint, table.clone()));
        held.truncate(self.capacity);
        Ok(table)
    }
}
