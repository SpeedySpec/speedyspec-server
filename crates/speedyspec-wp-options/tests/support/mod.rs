//! A Rust-only options API over the memory repository and a first-level object cache, with a hook log.

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_wp_hooks::{Callback, HookApi, HookName, HookValue, IgnoreDeprecations, Key};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_object_cache::{ObjectCache, ObjectCachePorts};
use speedyspec_wp_options::Option as OptionEntity;
use speedyspec_wp_options::{
    Autoload, InstallingFlag, OptionName, OptionRepository, OptionsApi, OptionsDeps,
    SerializedValue,
};
use speedyspec_wp_options_memory::MemoryOptionRepository;

pub type Log = Rc<RefCell<Vec<(String, Vec<HookValue>)>>>;

pub struct Fixture {
    pub api: OptionsApi,
    pub hooks: Rc<HookApi>,
    pub cache: Rc<ObjectCache>,
    pub repo: Rc<MemoryOptionRepository>,
    pub installing: Rc<InstallingFlag>,
    pub log: Log,
}

pub fn fixture() -> Fixture {
    let hooks = Rc::new(HookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let cache = Rc::new(ObjectCache::new(ObjectCachePorts::rust(), None));
    let repo = Rc::new(MemoryOptionRepository::new());
    let installing = Rc::new(InstallingFlag::new());
    let mut deps = OptionsDeps::rust(hooks.clone(), cache.clone(), repo.clone());
    deps.installing = installing.clone();
    Fixture {
        api: OptionsApi::new(deps),
        hooks,
        cache,
        repo,
        installing,
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
