//! The in-memory hook store.
//!
//! Callbacks registered before the first request (or between requests) belong to the worker partition and live as
//! long as the process; callbacks registered between [`MemoryHookStore::start_request`] and
//! [`MemoryHookStore::end_request`] belong to the request partition. Ending a request drops the request partition
//! and everything else a request owns: the run counts, the current-hook stack, hooks left with no callbacks, and any
//! hook running state left by a failed callback. The next request sees what a fresh WordPress request would after
//! the worker's own registrations.

use std::cell::{Cell, RefCell};

use indexmap::IndexMap;
use speedyspec_wp_hooks::{
    ArrayKey, Count, HookKey, HookStore, HookValue, Partition, Prehashed, WpHook,
};

/// A table keyed by hook name, by the hash the name carries (PHP's own for a name from PHP): a lookup never
/// hashes the name again.
type Table<V> = IndexMap<HookKey, V, Prehashed>;

#[derive(Default)]
struct Globals {
    filter: Table<WpHook>,
    actions: Table<i64>,
    filters: Table<i64>,
    current: Vec<HookValue>,
}

pub struct MemoryHookStore {
    globals: RefCell<Globals>,
    partition: Cell<Partition>,
}

impl Default for MemoryHookStore {
    fn default() -> Self {
        MemoryHookStore::new()
    }
}

impl MemoryHookStore {
    pub fn new() -> MemoryHookStore {
        MemoryHookStore {
            globals: RefCell::new(Globals::default()),
            partition: Cell::new(Partition::Worker),
        }
    }

    /// Registrations from now on belong to the request.
    pub fn start_request(&self) {
        self.partition.set(Partition::Request);
    }

    /// Drops the request partition; registrations from now on belong to the worker.
    pub fn end_request(&self) {
        let hooks: Vec<WpHook> = self.globals.borrow().filter.values().cloned().collect();
        for hook in &hooks {
            hook.drop_partition(Partition::Request);
        }
        let mut globals = self.globals.borrow_mut();
        globals.filter.retain(|_, hook| hook.has_filters());
        globals.actions.clear();
        globals.filters.clear();
        globals.current.clear();
        drop(globals);
        self.partition.set(Partition::Worker);
    }

    fn table(globals: &mut Globals, kind: Count) -> &mut Table<i64> {
        match kind {
            Count::Actions => &mut globals.actions,
            Count::Filters => &mut globals.filters,
        }
    }
}

impl HookStore for MemoryHookStore {
    fn hook(&self, name: &HookKey) -> Option<WpHook> {
        self.globals.borrow().filter.get(name).cloned()
    }

    fn has_hook(&self, name: &HookKey) -> bool {
        self.globals.borrow().filter.contains_key(name)
    }

    fn hooks(&self) -> Vec<(ArrayKey, WpHook)> {
        self.globals
            .borrow()
            .filter
            .iter()
            .map(|(k, v)| (k.key().clone(), v.clone()))
            .collect()
    }

    fn counts(&self, kind: Count) -> Vec<(ArrayKey, i64)> {
        let mut globals = self.globals.borrow_mut();
        Self::table(&mut globals, kind)
            .iter()
            .map(|(k, v)| (k.key().clone(), *v))
            .collect()
    }

    fn count(&self, kind: Count, name: &HookKey) -> Option<i64> {
        let mut globals = self.globals.borrow_mut();
        Self::table(&mut globals, kind).get(name).copied()
    }

    fn current_stack(&self) -> Vec<HookValue> {
        self.globals.borrow().current.clone()
    }

    fn partition(&self) -> Partition {
        self.partition.get()
    }

    fn insert(&self, name: HookKey, hook: WpHook) {
        self.globals.borrow_mut().filter.insert(name, hook);
    }

    fn remove(&self, name: &HookKey) {
        self.globals.borrow_mut().filter.shift_remove(name);
    }

    fn clear(&self) {
        self.globals.borrow_mut().filter.clear();
    }

    fn push_current(&self, name: HookValue) {
        self.globals.borrow_mut().current.push(name);
    }

    fn pop_current(&self) {
        self.globals.borrow_mut().current.pop();
    }

    fn set_current_stack(&self, stack: Vec<HookValue>) {
        self.globals.borrow_mut().current = stack;
    }

    fn increment_did(&self, kind: Count, name: &HookKey) {
        let mut globals = self.globals.borrow_mut();
        let table = Self::table(&mut globals, kind);
        // Only a hook run for the first time clones its name.
        match table.get_mut(name) {
            Some(n) => *n += 1,
            None => {
                table.insert(name.clone(), 1);
            }
        }
    }

    fn set_count(&self, kind: Count, name: &HookKey, value: Option<i64>) {
        let mut globals = self.globals.borrow_mut();
        let table = Self::table(&mut globals, kind);
        match value {
            Some(n) => {
                table.insert(name.clone(), n);
            }
            None => {
                table.shift_remove(name);
            }
        }
    }
}
