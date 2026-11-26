//! A Rust-only options API over the memory repository and a first-level object cache, with a hook log.

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_wp_hooks::{IgnoreDeprecations, WpHookApi};
use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue, Key};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_object_cache::{CurrentObjectCache, ObjectCachePorts, WpObjectCache};
use speedyspec_wp_options::Option as OptionEntity;
use speedyspec_wp_options::{
    Autoload, Deprecations, InstallingFlag, OptionName, OptionRepository, OptionsApi, OptionsDeps,
    OptionsError, SerializedValue,
};
use speedyspec_wp_options_memory::MemoryOptionRepository;

pub type Log = Rc<RefCell<Vec<(String, Vec<HookValue>)>>>;

/// `_deprecated_argument()` calls: function, version, message.
#[derive(Default)]
pub struct RecordedDeprecations(pub RefCell<Vec<(String, String, String)>>);

impl Deprecations for RecordedDeprecations {
    fn deprecated_argument(
        &self,
        function: &str,
        version: &str,
        message: &str,
    ) -> Result<(), OptionsError> {
        self.0
            .borrow_mut()
            .push((function.into(), version.into(), message.into()));
        Ok(())
    }

    fn doing_it_wrong(
        &self,
        function: &str,
        message: &str,
        version: &str,
    ) -> Result<(), OptionsError> {
        self.0
            .borrow_mut()
            .push((function.into(), version.into(), message.into()));
        Ok(())
    }
}

pub struct Fixture {
    pub api: OptionsApi,
    pub hooks: Rc<WpHookApi>,
    pub cache: Rc<WpObjectCache>,
    pub current: Rc<CurrentObjectCache>,
    pub repo: Rc<MemoryOptionRepository>,
    pub installing: Rc<InstallingFlag>,
    pub deprecations: Rc<RecordedDeprecations>,
    pub log: Log,
}

pub fn fixture() -> Fixture {
    let hooks = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let current = Rc::new(CurrentObjectCache::new(ObjectCachePorts::rust(), None));
    let cache = current.current();
    let repo = Rc::new(MemoryOptionRepository::new());
    let installing = Rc::new(InstallingFlag::new());
    let deprecations = Rc::new(RecordedDeprecations::default());
    let mut deps = OptionsDeps::rust(hooks.clone(), current.clone(), repo.clone());
    deps.installing = installing.clone();
    deps.deprecations = deprecations.clone();
    Fixture {
        api: OptionsApi::new(deps),
        hooks,
        cache,
        current,
        repo,
        installing,
        deprecations,
        log: Rc::new(RefCell::new(Vec::new())),
    }
}

/// Logs each run of `hook` with its arguments; the callback returns its first argument.
pub fn record(f: &Fixture, hook: &str) {
    let log = f.log.clone();
    let name = hook.to_string();
    f.hooks
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

/// Adds a callback that answers `value` whatever it gets.
pub fn answer(f: &Fixture, hook: &str, value: HookValue) {
    f.hooks
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

pub fn hooks_run(f: &Fixture) -> Vec<String> {
    f.log.borrow().iter().map(|(h, _)| h.clone()).collect()
}

pub fn seed(f: &Fixture, name: &str, value: &str, autoload: Autoload) {
    f.repo
        .add(&OptionEntity {
            name: OptionName::new(name).unwrap(),
            value: SerializedValue::new(value.as_bytes().to_vec()),
            autoload,
        })
        .unwrap();
}

pub fn s(v: &str) -> HookValue {
    HookValue::Str(v.into())
}

pub fn options() -> HookValue {
    s("options")
}

pub fn assoc(pairs: &[(&str, HookValue)]) -> HookValue {
    HookValue::Array(
        pairs
            .iter()
            .map(|(k, v)| (Key::Str((*k).into()), v.clone()))
            .collect(),
    )
}

pub fn cached(f: &Fixture, key: &str) -> Option<HookValue> {
    f.cache.get(&s(key), &options(), false).unwrap()
}

/// `$alloptions[$name]` as the cache holds it.
pub fn in_alloptions(f: &Fixture, name: &str) -> Option<HookValue> {
    match cached(f, "alloptions") {
        Some(HookValue::Array(items)) => items
            .into_iter()
            .find(|(k, _)| *k == Key::Str(name.into()))
            .map(|(_, v)| v),
        _ => None,
    }
}

pub fn stored(f: &Fixture, name: &str) -> Option<Vec<u8>> {
    f.repo
        .value(&OptionName::new(name).unwrap(), false)
        .unwrap()
        .map(|v| v.as_bytes().to_vec())
}
