use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_routing::{DispatchStrategy, Fingerprint, RouteData, RouteTableStore};

use crate::{CompileError, CompiledTable};

/// Holds the worker's current compiled table and rebuilds it only when its fingerprint changes.
///
/// On a change it asks the store for the route data, builds and stores it on a miss, compiles it, and keeps the
/// result. Nothing passes between workers or replicas: each one compiles its own table from its own inputs.
pub struct TableCache {
    store: Rc<dyn RouteTableStore>,
    strategy: Rc<dyn DispatchStrategy>,
    current: RefCell<Option<(Fingerprint, Rc<CompiledTable>)>>,
    store_error_logged: Cell<bool>,
}

impl TableCache {
    pub fn new(store: Rc<dyn RouteTableStore>, strategy: Rc<dyn DispatchStrategy>) -> TableCache {
        TableCache {
            store,
            strategy,
            current: RefCell::new(None),
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
        if let Some((held, table)) = self.current.borrow().as_ref()
            && *held == fingerprint
        {
            return Ok(table.clone());
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
        *self.current.borrow_mut() = Some((fingerprint, table.clone()));
        Ok(table)
    }
}
