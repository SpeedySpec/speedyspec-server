//! [`MemoryCronStore`]: the `cron` option in memory, for tests. It can stand in for another process's writes.

use std::cell::{Cell, RefCell};

use speedyspec_php_serialize::{from_hook_value, serialize};
use speedyspec_wp_cron_contracts::{CronError, CronStore, SaveOutcome, Version};
use speedyspec_wp_hooks::HookValue;

type BeforeSave = Box<dyn FnOnce(&MemoryCronStore)>;

/// The option's value (`None`: absent), its versions as serialized bytes.
#[derive(Default)]
pub struct MemoryCronStore {
    raw: RefCell<Option<HookValue>>,
    writes: Cell<u64>,
    forced_conflicts: Cell<u32>,
    reload_filtered: Cell<bool>,
    unconditional: Cell<u64>,
    before_save: RefCell<Option<BeforeSave>>,
}

fn version(raw: &Option<HookValue>) -> Version {
    Version(raw.as_ref().map(|v| match from_hook_value(&v.read()) {
        Ok(php) => serialize(&php),
        Err(_) => format!("{v:?}").into_bytes(),
    }))
}

impl MemoryCronStore {
    pub fn new() -> MemoryCronStore {
        MemoryCronStore::default()
    }

    /// Replaces the option behind every reader, as another process's write does.
    pub fn set_raw(&self, value: Option<HookValue>) {
        *self.raw.borrow_mut() = value;
    }

    pub fn raw(&self) -> Option<HookValue> {
        self.raw.borrow().clone()
    }

    /// Saves that landed.
    pub fn writes(&self) -> u64 {
        self.writes.get()
    }

    /// The next `n` compare-and-swap saves conflict whatever they expect.
    pub fn force_conflicts(&self, n: u32) {
        self.forced_conflicts.set(n);
    }

    /// Runs `f` once, just before the next save compares: another process acting between a read and a write.
    pub fn before_next_save(&self, f: impl FnOnce(&MemoryCronStore) + 'static) {
        *self.before_save.borrow_mut() = Some(Box::new(f));
    }

    /// `reload()` reports that a filter rewrote the value read, so a compare-and-swap against it cannot succeed.
    pub fn reload_reports_filtered(&self, filtered: bool) {
        self.reload_filtered.set(filtered);
    }

    /// Unconditional saves (`save`) that landed.
    pub fn unconditional_saves(&self) -> u64 {
        self.unconditional.get()
    }

    fn run_before_save(&self) {
        let pending = self.before_save.borrow_mut().take();
        if let Some(f) = pending {
            f(self);
        }
    }

    fn current(&self) -> (HookValue, Version) {
        let raw = self.raw.borrow().clone();
        let v = version(&raw);
        (raw.unwrap_or(HookValue::Bool(false)), v)
    }
}

impl CronStore for MemoryCronStore {
    fn load(&self) -> Result<(HookValue, Version), CronError> {
        Ok(self.current())
    }

    fn reload(&self) -> Result<(HookValue, Version, bool), CronError> {
        let (value, v) = self.current();
        Ok((value, v, !self.reload_filtered.get()))
    }

    fn save_if(&self, expected: &Version, value: HookValue) -> Result<SaveOutcome, CronError> {
        self.run_before_save();
        if self.forced_conflicts.get() > 0 {
            self.forced_conflicts.set(self.forced_conflicts.get() - 1);
            return Ok(SaveOutcome::Conflict);
        }
        let current = self.raw.borrow().clone();
        if &version(&current) != expected {
            return Ok(SaveOutcome::Conflict);
        }
        if version(&Some(value.clone())) == version(&current) {
            return Ok(SaveOutcome::Unchanged);
        }
        *self.raw.borrow_mut() = Some(value);
        self.writes.set(self.writes.get() + 1);
        Ok(SaveOutcome::Written)
    }

    fn save(&self, value: HookValue) -> Result<bool, CronError> {
        self.run_before_save();
        let current = self.raw.borrow().clone();
        if version(&Some(value.clone())) == version(&current) {
            return Ok(false);
        }
        *self.raw.borrow_mut() = Some(value);
        self.writes.set(self.writes.get() + 1);
        self.unconditional.set(self.unconditional.get() + 1);
        Ok(true)
    }
}
