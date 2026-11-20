//! `MemoryDirectStore`: named byte blobs, for test repositories over a Direct Store.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use speedyspec_data::{DbError, DirectStore, Health};

pub struct MemoryDirectStore {
    engine: String,
    connected: Cell<bool>,
    blobs: RefCell<HashMap<String, Vec<u8>>>,
}

impl MemoryDirectStore {
    pub fn new(engine: &str) -> MemoryDirectStore {
        MemoryDirectStore {
            engine: engine.to_string(),
            connected: Cell::new(false),
            blobs: RefCell::new(HashMap::new()),
        }
    }

    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        self.blobs.borrow().get(name).cloned()
    }

    pub fn put(&self, name: &str, bytes: &[u8]) {
        self.blobs
            .borrow_mut()
            .insert(name.to_string(), bytes.to_vec());
    }

    pub fn remove(&self, name: &str) -> bool {
        self.blobs.borrow_mut().remove(name).is_some()
    }
}

impl DirectStore for MemoryDirectStore {
    fn engine(&self) -> &str {
        &self.engine
    }

    fn kind(&self) -> &'static str {
        "memory"
    }

    fn connect(&self) -> Result<(), DbError> {
        self.connected.set(true);
        Ok(())
    }

    fn health(&self) -> Result<Health, DbError> {
        let ok = self.connected.get();
        Ok(Health {
            ok,
            detail: if ok {
                "connected".into()
            } else {
                "closed".into()
            },
        })
    }

    fn close(&self) -> Result<(), DbError> {
        self.connected.set(false);
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
