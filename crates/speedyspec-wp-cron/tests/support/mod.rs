//! A `CronApi` over `MemoryCronStore`, with a fixed clock and Rust stand-ins for the PHP ports.
#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_wp_cron::memory::{
    CollectingErrors, MemoryCronStore, MemoryDoingCron, RecordingWake,
};
use speedyspec_wp_cron::{CronApi, CronParts, RustArgsKeyer};
use speedyspec_wp_cron_contracts::{Clock, PhpObjects, Translator};
use speedyspec_wp_hooks::{IgnoreDeprecations, WpHookApi};
use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue, Key};
use speedyspec_wp_hooks_memory::MemoryHookStore;

pub const NOW: i64 = 1_700_000_000;

/// Each hook run a test recorded, with its arguments.
pub type Log = Rc<RefCell<Vec<(String, Vec<HookValue>)>>>;

pub struct FixedClock(pub Cell<f64>);

impl Clock for FixedClock {
    fn microtime(&self) -> f64 {
        self.0.get()
    }
    fn time(&self) -> i64 {
        self.0.get() as i64
    }
    fn pause(&self, _millis: u64) {}
}

pub struct SameText;

impl Translator for SameText {
    fn translate(&self, text: &str) -> String {
        text.to_string()
    }
}

/// Objects are arrays tagged `__object`; a `WP_Error` is an array tagged `__wp_error`.
pub struct ArrayObjects;

impl PhpObjects for ArrayObjects {
    fn to_object(&self, array: HookValue) -> HookValue {
        match array {
            HookValue::Array(mut items) => {
                items.insert(0, (Key::Str("__object".into()), HookValue::Bool(true)));
                HookValue::Array(items)
            }
            other => other,
        }
    }
    fn property(&self, object: &HookValue, name: &str) -> Option<HookValue> {
        match object {
            HookValue::Array(items)
                if items
                    .first()
                    .is_some_and(|(k, _)| *k == Key::Str("__object".into())) =>
            {
                items
                    .iter()
                    .find(|(k, _)| *k == Key::Str(name.into()))
                    .map(|(_, v)| v.clone())
            }
            _ => None,
        }
    }
    fn is_wp_error(&self, value: &HookValue) -> bool {
        matches!(value, HookValue::Array(items) if items.first().is_some_and(|(k, _)| *k == Key::Str("__wp_error".into())))
    }
}

pub fn wp_error() -> HookValue {
    HookValue::Array(vec![(Key::Str("__wp_error".into()), HookValue::Bool(true))])
}

pub struct Fixture {
    pub api: CronApi,
    pub store: Rc<MemoryCronStore>,
    pub wake: Rc<RecordingWake>,
    pub doing: Rc<MemoryDoingCron>,
    pub errors: Rc<CollectingErrors>,
    pub hooks: Rc<WpHookApi>,
    pub clock: Rc<FixedClock>,
    pub log: Log,
}

pub fn fixture() -> Fixture {
    fixture_over(Rc::new(MemoryCronStore::new()))
}

/// A fixture over `store`: two fixtures over one store are two processes sharing one `cron` option.
pub fn fixture_over(store: Rc<MemoryCronStore>) -> Fixture {
    let hooks = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let wake = Rc::new(RecordingWake::new());
    let doing = Rc::new(MemoryDoingCron::new());
    let errors = Rc::new(CollectingErrors::new());
    let clock = Rc::new(FixedClock(Cell::new(NOW as f64 + 0.5)));
    let api = CronApi::new(CronParts {
        hooks: hooks.clone(),
        store: store.clone(),
        keyer: Rc::new(RustArgsKeyer),
        clock: clock.clone(),
        translator: Rc::new(SameText),
        objects: Rc::new(ArrayObjects),
        wake: wake.clone(),
        doing_cron: doing.clone(),
        network: Rc::new(speedyspec_wp_cron::memory::SingleSite),
        switcher: Rc::new(speedyspec_wp_cron::memory::SingleSite),
        errors: errors.clone(),
    });
    Fixture {
        api,
        store,
        wake,
        doing,
        errors,
        hooks,
        clock,
        log: Rc::new(RefCell::new(Vec::new())),
    }
}

impl Fixture {
    /// Answers `value` on `hook`, whatever it gets.
    pub fn answer(&self, hook: &str, value: HookValue) {
        self.hooks
            .add_filter(
                &HookName::from(hook),
                Callback::rust(&format!("answer_{hook}"), move |_: &[HookValue]| {
                    Ok(value.clone())
                }),
                &HookValue::Int(10),
                99,
            )
            .unwrap();
    }

    /// Logs each run of `hook` with its arguments; returns its first argument.
    pub fn record(&self, hook: &str) {
        let log = self.log.clone();
        let name = hook.to_string();
        self.hooks
            .add_filter(
                &HookName::from(hook),
                Callback::rust(&format!("record_{hook}"), move |args: &[HookValue]| {
                    log.borrow_mut().push((name.clone(), args.to_vec()));
                    Ok(args.first().cloned().unwrap_or(HookValue::Null))
                }),
                &HookValue::Int(10),
                99,
            )
            .unwrap();
    }

    pub fn ran(&self) -> Vec<String> {
        self.log.borrow().iter().map(|(h, _)| h.clone()).collect()
    }
}

pub fn s(v: &str) -> HookValue {
    HookValue::Str(v.into())
}

pub fn arr(items: Vec<(Key, HookValue)>) -> HookValue {
    HookValue::Array(items)
}

pub fn list(items: Vec<HookValue>) -> HookValue {
    HookValue::Array(
        items
            .into_iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v))
            .collect(),
    )
}

pub fn assoc(pairs: &[(&str, HookValue)]) -> HookValue {
    HookValue::Array(
        pairs
            .iter()
            .map(|(k, v)| (Key::Str((*k).into()), v.clone()))
            .collect(),
    )
}

/// `md5( serialize( array() ) )`.
pub const EMPTY_ARGS_KEY: &str = "40cd750bba9870f18aada2478b24840a";

/// A cron option (with `version` 2) holding one single event per `(timestamp, hook)`, empty args.
pub fn option_with(events: &[(i64, &str)]) -> HookValue {
    let mut items: Vec<(Key, HookValue)> = Vec::new();
    for (ts, hook) in events {
        items.push((
            Key::Int(*ts),
            assoc(&[(
                hook,
                assoc(&[(
                    EMPTY_ARGS_KEY,
                    assoc(&[("schedule", HookValue::Bool(false)), ("args", arr(vec![]))]),
                )]),
            )]),
        ));
    }
    items.push((Key::Str("version".into()), HookValue::Int(2)));
    HookValue::Array(items)
}

/// The value `$object->$name` of an `ArrayObjects` object.
pub fn prop(object: &HookValue, name: &str) -> Option<HookValue> {
    ArrayObjects.property(object, name)
}
