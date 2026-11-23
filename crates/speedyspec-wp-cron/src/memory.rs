//! In-memory stand-ins for tests: [`MemoryCronStore`] (the `cron` option, which can stand in for another process's
//! writes), [`RecordingWake`], [`MemoryDoingCron`] and [`CollectingErrors`].

use std::cell::{Cell, RefCell};

use speedyspec_php_serialize::{from_hook_value, serialize};
use speedyspec_wp_cron_contracts::{
    CronError, CronResult, CronStep, CronStore, DoingCron, ErrorReporter, NetworkSites,
    SaveOutcome, SiteRef, SiteSwitcher, Version, WakeReporter,
};
use speedyspec_wp_hooks::HookValue;
use speedyspec_wp_hooks::Key;

type BeforeSave = Box<dyn FnOnce(&MemoryCronStore)>;

/// The option's value (`None`: absent), its versions as serialized bytes.
#[derive(Default)]
pub struct MemoryCronStore {
    raw: RefCell<Option<HookValue>>,
    writes: Cell<u64>,
    forced_conflicts: Cell<u32>,
    loads: Cell<u32>,
    before_load: RefCell<Option<(u32, BeforeSave)>>,
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

    /// Runs `f` once, just before load number `n` (counting from 1) reads: another process writing between two
    /// reads.
    pub fn before_load(&self, n: u32, f: impl FnOnce(&MemoryCronStore) + 'static) {
        *self.before_load.borrow_mut() = Some((self.loads.get() + n, Box::new(f)));
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
        self.loads.set(self.loads.get() + 1);
        let due = matches!(&*self.before_load.borrow(), Some((n, _)) if *n == self.loads.get());
        if due && let Some((_, f)) = self.before_load.borrow_mut().take() {
            f(self);
        }
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

/// Keeps the earliest wake time reported.
#[derive(Default)]
pub struct RecordingWake(Cell<Option<i64>>);

impl RecordingWake {
    pub fn new() -> RecordingWake {
        RecordingWake::default()
    }

    pub fn earliest(&self) -> Option<i64> {
        self.0.get()
    }
}

impl WakeReporter for RecordingWake {
    fn wake_at(&self, timestamp: i64) {
        self.0
            .set(Some(self.0.get().map_or(timestamp, |t| t.min(timestamp))));
    }
}

/// The `doing_cron` transient in memory, with every change kept.
#[derive(Default)]
pub struct MemoryDoingCron {
    value: RefCell<Option<String>>,
    history: RefCell<Vec<Option<String>>>,
}

impl MemoryDoingCron {
    pub fn new() -> MemoryDoingCron {
        MemoryDoingCron::default()
    }

    /// Each set (`Some`) and delete (`None`), in order.
    pub fn history(&self) -> Vec<Option<String>> {
        self.history.borrow().clone()
    }
}

impl DoingCron for MemoryDoingCron {
    fn get(&self) -> Result<Option<String>, CronError> {
        Ok(self.value.borrow().clone())
    }

    fn set(&self, value: &str) -> Result<(), CronError> {
        *self.value.borrow_mut() = Some(value.to_string());
        self.history.borrow_mut().push(Some(value.to_string()));
        Ok(())
    }

    fn delete(&self) -> Result<(), CronError> {
        *self.value.borrow_mut() = None;
        self.history.borrow_mut().push(None);
        Ok(())
    }
}

/// Keeps each reported failure as (step, hook, code); the error handed to the action is an array tagged
/// `__wp_error` with the code.
#[derive(Default)]
pub struct CollectingErrors(RefCell<Vec<(CronStep, String, String)>>);

impl CollectingErrors {
    pub fn new() -> CollectingErrors {
        CollectingErrors::default()
    }

    pub fn reported(&self) -> Vec<(CronStep, String, String)> {
        self.0.borrow().clone()
    }
}

impl ErrorReporter for CollectingErrors {
    fn report(
        &self,
        step: CronStep,
        hook: &HookValue,
        error: &CronResult,
        _event: &HookValue,
    ) -> HookValue {
        let code = match error {
            CronResult::Error { code, .. } => (*code).to_string(),
            _ => "value".to_string(),
        };
        let hook = match hook {
            HookValue::Str(s) => s.clone(),
            other => format!("{other:?}"),
        };
        self.0.borrow_mut().push((step, hook, code.clone()));
        HookValue::Array(vec![
            (Key::Str("__wp_error".into()), HookValue::Bool(true)),
            (Key::Str("code".into()), HookValue::Str(code)),
        ])
    }
}

/// A single-site install: not multisite, nothing to switch.
pub struct SingleSite;

impl NetworkSites for SingleSite {
    fn is_multisite(&self) -> bool {
        false
    }
    fn sites(&self) -> Result<Vec<SiteRef>, CronError> {
        Ok(Vec::new())
    }
    fn current(&self) -> u64 {
        1
    }
}

impl SiteSwitcher for SingleSite {
    fn switch_to(&self, _: u64) -> Result<(), CronError> {
        Ok(())
    }
    fn restore(&self) -> Result<(), CronError> {
        Ok(())
    }
}

/// A network in memory: each site its own [`MemoryCronStore`] (domain `site<id>.test`, path `/`), the `cron` store
/// answering for the current site, and a record of every switch.
pub struct MemoryNetwork {
    stores: Vec<(u64, std::rc::Rc<MemoryCronStore>)>,
    current: Cell<u64>,
    stack: RefCell<Vec<u64>>,
    switches: RefCell<Vec<u64>>,
    failing: Cell<Option<u64>>,
}

impl MemoryNetwork {
    /// A network of `sites`, the first one current.
    pub fn new(sites: &[u64]) -> MemoryNetwork {
        MemoryNetwork {
            stores: sites
                .iter()
                .map(|&s| (s, std::rc::Rc::new(MemoryCronStore::new())))
                .collect(),
            current: Cell::new(sites.first().copied().unwrap_or(1)),
            stack: RefCell::new(Vec::new()),
            switches: RefCell::new(Vec::new()),
            failing: Cell::new(None),
        }
    }

    pub fn store(&self, site: u64) -> std::rc::Rc<MemoryCronStore> {
        self.stores
            .iter()
            .find(|(s, _)| *s == site)
            .map(|(_, store)| store.clone())
            .expect("a site of this network")
    }

    /// Switches not yet restored.
    pub fn depth(&self) -> usize {
        self.stack.borrow().len()
    }

    /// Every site switched to, in order.
    pub fn switches(&self) -> Vec<u64> {
        self.switches.borrow().clone()
    }

    /// Switching to `site` fails after the switch, as a throwing `switch_blog` callback does.
    pub fn fail_switch_to(&self, site: u64) {
        self.failing.set(Some(site));
    }

    fn here(&self) -> std::rc::Rc<MemoryCronStore> {
        self.store(self.current.get())
    }
}

impl CronStore for MemoryNetwork {
    fn load(&self) -> Result<(HookValue, Version), CronError> {
        self.here().load()
    }
    fn reload(&self) -> Result<(HookValue, Version, bool), CronError> {
        self.here().reload()
    }
    fn save_if(&self, expected: &Version, value: HookValue) -> Result<SaveOutcome, CronError> {
        self.here().save_if(expected, value)
    }
    fn save(&self, value: HookValue) -> Result<bool, CronError> {
        self.here().save(value)
    }
}

impl NetworkSites for MemoryNetwork {
    fn is_multisite(&self) -> bool {
        true
    }
    fn sites(&self) -> Result<Vec<SiteRef>, CronError> {
        Ok(self
            .stores
            .iter()
            .map(|(id, _)| SiteRef {
                id: *id,
                domain: format!("site{id}.test"),
                path: "/".to_string(),
            })
            .collect())
    }
    fn current(&self) -> u64 {
        self.current.get()
    }
}

impl SiteSwitcher for MemoryNetwork {
    fn switch_to(&self, site: u64) -> Result<(), CronError> {
        self.stack.borrow_mut().push(self.current.get());
        self.current.set(site);
        self.switches.borrow_mut().push(site);
        if self.failing.get() == Some(site) {
            return Err(CronError::new("switch_blog failed"));
        }
        Ok(())
    }
    fn restore(&self) -> Result<(), CronError> {
        if let Some(previous) = self.stack.borrow_mut().pop() {
            self.current.set(previous);
        }
        Ok(())
    }
}
