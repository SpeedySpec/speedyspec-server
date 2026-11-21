#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_wp_hooks::{Callback, HookApi, HookName, HookValue, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_rewrite::memory::{
    FixedHomeUrl, FlagWpLoaded, MemoryPageUris, MemoryPostTypes, MemoryQueryVars,
    MemoryRewriteOptions, MemoryRewriteStateSource,
};
use speedyspec_wp_rewrite::{RewriteDeps, RewriteService, RewriteState};

pub type Log = Rc<RefCell<Vec<(String, Vec<HookValue>)>>>;

pub struct Fixture {
    pub service: RewriteService,
    pub options: Rc<MemoryRewriteOptions>,
    pub hooks: Rc<HookApi>,
    pub loaded: Rc<FlagWpLoaded>,
    pub query_vars: Rc<MemoryQueryVars>,
    pub source: Rc<MemoryRewriteStateSource>,
    pub log: Log,
}

pub fn fixture(permalink_structure: &str) -> Fixture {
    let hooks = Rc::new(HookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let options = Rc::new(MemoryRewriteOptions::new());
    options.set("permalink_structure", permalink_structure);
    let loaded = Rc::new(FlagWpLoaded::new(true));
    let query_vars = Rc::new(MemoryQueryVars::default());
    let deps = RewriteDeps {
        options: options.clone(),
        home: Rc::new(FixedHomeUrl("https://example.test".into())),
        loaded: loaded.clone(),
        pages: Rc::new(MemoryPageUris::default()),
        post_types: Rc::new(MemoryPostTypes::default()),
        query_vars: query_vars.clone(),
        hooks: hooks.clone(),
    };
    let service = RewriteService::new(deps);
    let source = Rc::new(MemoryRewriteStateSource::new(RewriteState::default()));
    Fixture {
        service,
        options,
        hooks,
        loaded,
        query_vars,
        source,
        log: Rc::default(),
    }
}

/// Logs each run of `hook`; the callback returns its first argument.
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

pub fn hooks_run(f: &Fixture) -> Vec<String> {
    f.log.borrow().iter().map(|(h, _)| h.clone()).collect()
}
