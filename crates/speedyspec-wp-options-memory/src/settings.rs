//! `MemorySettingsStore`: `$wp_registered_settings` and `$new_allowed_options` in memory, PHP's array indices kept.

use std::cell::RefCell;

use indexmap::IndexMap;
use speedyspec_wp_hooks::HookValue;
use speedyspec_wp_options::{OptionGroups, SettingsStore};

/// A group's allowed names: PHP's next free index, and each entry with its index.
type Allowed = (i64, Vec<(i64, String)>);

#[derive(Default)]
pub struct MemorySettingsStore {
    registered: RefCell<IndexMap<String, HookValue>>,
    allowed: RefCell<IndexMap<String, Allowed>>,
}

impl MemorySettingsStore {
    pub fn new() -> MemorySettingsStore {
        MemorySettingsStore::default()
    }

    /// `$new_allowed_options[ $group ]`, index and name.
    pub fn allowed(&self, group: &str) -> Vec<(i64, String)> {
        self.allowed
            .borrow()
            .get(group)
            .map(|(_, names)| names.clone())
            .unwrap_or_default()
    }
}

impl SettingsStore for MemorySettingsStore {
    fn registered(&self) -> IndexMap<String, HookValue> {
        self.registered.borrow().clone()
    }

    fn get(&self, name: &str) -> Option<HookValue> {
        self.registered.borrow().get(name).cloned()
    }

    fn put(&self, name: &str, args: HookValue) {
        self.registered.borrow_mut().insert(name.to_string(), args);
    }

    fn remove(&self, name: &str) {
        self.registered.borrow_mut().shift_remove(name);
    }

    fn allow(&self, group: &str, name: &str) {
        let mut allowed = self.allowed.borrow_mut();
        let (next, names) = allowed.entry(group.to_string()).or_default();
        names.push((*next, name.to_string()));
        *next += 1;
    }

    fn disallow(&self, group: &str, name: &str) {
        if let Some((_, names)) = self.allowed.borrow_mut().get_mut(group)
            && let Some(at) = names.iter().position(|(_, n)| n == name)
        {
            names.remove(at);
        }
    }
}

impl OptionGroups for MemorySettingsStore {
    fn options_in(&self, group: &str) -> Option<Vec<String>> {
        self.allowed
            .borrow()
            .get(group)
            .map(|(_, names)| names.iter().map(|(_, n)| n.clone()).collect())
    }
}
