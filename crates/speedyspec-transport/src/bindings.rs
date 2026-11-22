//! An in-memory binding store.

use std::cell::RefCell;
use std::collections::BTreeMap;

use speedyspec_operation_contracts::Value;
use speedyspec_transport_contracts::BindingStore;

/// Bindings in memory, keyed by operation, then transport.
#[derive(Default)]
pub struct MemoryBindingStore {
    bindings: RefCell<BTreeMap<(String, String), Value>>,
}

impl MemoryBindingStore {
    pub fn new() -> Self {
        MemoryBindingStore::default()
    }
}

impl BindingStore for MemoryBindingStore {
    fn bind(&self, operation: &str, transport: &str, config: Value) {
        self.bindings
            .borrow_mut()
            .insert((operation.to_owned(), transport.to_owned()), config);
    }

    fn binding(&self, operation: &str, transport: &str) -> Option<Value> {
        self.bindings
            .borrow()
            .get(&(operation.to_owned(), transport.to_owned()))
            .cloned()
    }

    fn bindings(&self) -> Vec<(String, String, Value)> {
        self.bindings
            .borrow()
            .iter()
            .map(|((op, transport), config)| (op.clone(), transport.clone(), config.clone()))
            .collect()
    }
}
