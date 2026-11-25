#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_wp_hooks::{Callback, HookApi, HookName, HookValue, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_rewrite::memory::{
    FixedUrls, FlagWpLoaded, MemoryPageUris, MemoryPostTypes, MemoryQueryVars,
    MemoryRewriteOptions, MemoryRewriteStateSource, MemoryTaxonomies, MemoryViewablePosts,
    SingleSite, UntranslatedSite,
};
use speedyspec_wp_rewrite::{
    FilteredRandomContentRedirects, RewriteDeps, RewriteService, RewriteState,
};

pub type Log = Rc<RefCell<Vec<(String, Vec<HookValue>)>>>;

pub struct Fixture {
    pub service: RewriteService,
    pub options: Rc<MemoryRewriteOptions>,
    pub hooks: Rc<HookApi>,
    pub loaded: Rc<FlagWpLoaded>,
    pub query_vars: Rc<MemoryQueryVars>,
    pub source: Rc<MemoryRewriteStateSource>,
    pub post_types: Rc<MemoryPostTypes>,
    pub taxonomies: Rc<MemoryTaxonomies>,
    pub posts: Rc<MemoryViewablePosts>,
    pub log: Log,
}

pub fn fixture(permalink_structure: &str) -> Fixture {
    fixture_with(
        permalink_structure,
        "https://example.test",
        "https://example.test",
    )
}

pub fn fixture_with(permalink_structure: &str, home: &str, site: &str) -> Fixture {
    let hooks = Rc::new(HookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let options = Rc::new(MemoryRewriteOptions::new());
    options.set("permalink_structure", permalink_structure);
    let loaded = Rc::new(FlagWpLoaded::new(true));
    let query_vars = Rc::new(MemoryQueryVars::default());
    let source = Rc::new(MemoryRewriteStateSource::new(RewriteState::default()));
    let post_types = Rc::new(MemoryPostTypes::default());
    let taxonomies = Rc::new(MemoryTaxonomies::default());
    let posts = Rc::new(MemoryViewablePosts::default());
    let deps = RewriteDeps {
        options: options.clone(),
        urls: Rc::new(FixedUrls {
            home: home.into(),
            site: site.into(),
        }),
        multisite: Rc::new(SingleSite),
        loaded: loaded.clone(),
        pages: Rc::new(MemoryPageUris::default()),
        post_types: post_types.clone(),
        taxonomies: taxonomies.clone(),
        posts: posts.clone(),
        random_content: Rc::new(FilteredRandomContentRedirects::new(hooks.clone())),
        translations: Rc::new(UntranslatedSite),
        query_vars: query_vars.clone(),
        hooks: hooks.clone(),
        state: source.clone(),
    };
    let service = RewriteService::new(deps);
    Fixture {
        service,
        options,
        hooks,
        loaded,
        query_vars,
        source,
        post_types,
        taxonomies,
        posts,
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
