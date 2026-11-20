//! An in-memory option repository, for tests and for callers with no database. It answers as MySQL's
//! `wp_options` does, row counts included.

mod sitemeta;
pub mod suites;

pub use sitemeta::MemorySitemetaRepository;

use std::cell::{Cell, RefCell};

use indexmap::IndexMap;
use speedyspec_wp_options::Option as OptionEntity;
use speedyspec_wp_options::{
    Autoload, OptionName, OptionRepository, OptionsError, SerializedValue,
};

/// `wp_options` in a map, in insertion order (the table's order).
#[derive(Default)]
pub struct MemoryOptionRepository {
    rows: RefCell<IndexMap<String, (SerializedValue, Autoload)>>,
    statements: Cell<u64>,
}

impl MemoryOptionRepository {
    pub fn new() -> MemoryOptionRepository {
        MemoryOptionRepository::default()
    }

    /// How many statements have been sent: one per method call, as `$wpdb` sends one per call.
    pub fn statements(&self) -> u64 {
        self.statements.get()
    }

    fn sent(&self) {
        self.statements.set(self.statements.get() + 1);
    }
}

impl OptionRepository for MemoryOptionRepository {
    fn engine(&self) -> String {
        "memory".into()
    }

    fn value(
        &self,
        name: &OptionName,
        _quiet: bool,
    ) -> Result<Option<SerializedValue>, OptionsError> {
        self.sent();
        Ok(self
            .rows
            .borrow()
            .get(name.as_str())
            .map(|(v, _)| v.clone()))
    }

    fn autoload_of(&self, name: &OptionName) -> Result<Option<Autoload>, OptionsError> {
        self.sent();
        Ok(self
            .rows
            .borrow()
            .get(name.as_str())
            .map(|(_, a)| a.clone()))
    }

    fn values_of(&self, names: &[String]) -> Result<Vec<(String, SerializedValue)>, OptionsError> {
        self.sent();
        Ok(self
            .rows
            .borrow()
            .iter()
            .filter(|(n, _)| names.contains(n))
            .map(|(n, (v, _))| (n.clone(), v.clone()))
            .collect())
    }

    fn autoloaded(
        &self,
        values: &[String],
    ) -> Result<Vec<(String, SerializedValue)>, OptionsError> {
        self.sent();
        Ok(self
            .rows
            .borrow()
            .iter()
            .filter(|(_, (_, a))| values.iter().any(|v| v == a.as_str()))
            .map(|(n, (v, _))| (n.clone(), v.clone()))
            .collect())
    }

    fn all(&self) -> Result<Vec<(String, SerializedValue)>, OptionsError> {
        self.sent();
        Ok(self
            .rows
            .borrow()
            .iter()
            .map(|(n, (v, _))| (n.clone(), v.clone()))
            .collect())
    }

    fn needing_autoload_change(
        &self,
        groups: &[(Autoload, Vec<String>)],
    ) -> Result<Vec<String>, OptionsError> {
        self.sent();
        let rows = self.rows.borrow();
        let mut changed = Vec::new();
        for (autoload, names) in groups {
            for n in names {
                if rows.get(n.as_str()).is_some_and(|(_, a)| a != autoload) {
                    changed.push(n.clone());
                }
            }
        }
        Ok(changed)
    }

    fn set_autoload(&self, names: &[String], autoload: &Autoload) -> Result<u64, OptionsError> {
        self.sent();
        let mut changed = 0;
        for (n, (_, a)) in self.rows.borrow_mut().iter_mut() {
            if names.contains(n) && a != autoload {
                *a = autoload.clone();
                changed += 1;
            }
        }
        Ok(changed)
    }

    fn add(&self, option: &OptionEntity) -> Result<u64, OptionsError> {
        self.sent();
        let mut rows = self.rows.borrow_mut();
        let row = (option.value.clone(), option.autoload.clone());
        match rows.get_mut(option.name.as_str()) {
            None => {
                rows.insert(option.name.as_str().to_string(), row);
                Ok(1)
            }
            Some(existing) if *existing == row => Ok(0),
            Some(existing) => {
                *existing = row;
                Ok(2)
            }
        }
    }

    /// A `NULL` value is stored as an empty one, as MySQL's non-strict mode does for the `NOT NULL` column.
    fn update(
        &self,
        name: &OptionName,
        value: Option<&SerializedValue>,
        autoload: Option<&Autoload>,
    ) -> Result<u64, OptionsError> {
        self.sent();
        let mut rows = self.rows.borrow_mut();
        let Some((v, a)) = rows.get_mut(name.as_str()) else {
            return Ok(0);
        };
        let new_value = value.cloned().unwrap_or_default();
        let new_autoload = autoload.cloned().unwrap_or_else(|| a.clone());
        if *v == new_value && *a == new_autoload {
            return Ok(0);
        }
        *v = new_value;
        *a = new_autoload;
        Ok(1)
    }

    fn remove(&self, name: &OptionName) -> Result<u64, OptionsError> {
        self.sent();
        Ok(u64::from(
            self.rows.borrow_mut().shift_remove(name.as_str()).is_some(),
        ))
    }
}
