//! In-memory implementations of the ports, for tests and Rust-only use.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_routing::Rule;
use speedyspec_wp_hooks::HookValue;

use crate::{
    Multisite, PageUris, PostTypes, PublicQueryVars, RewriteError, RewriteOptions, RewriteState,
    RewriteStateSource, SiteUrls, UriIndex, WpLoaded,
};

/// Options in a map; `rewrite_rules` kept as rules.
#[derive(Default)]
pub struct MemoryRewriteOptions {
    values: RefCell<HashMap<String, String>>,
    rules: RefCell<Option<Vec<Rule>>>,
}

impl MemoryRewriteOptions {
    pub fn new() -> MemoryRewriteOptions {
        MemoryRewriteOptions::default()
    }

    pub fn set(&self, name: &str, value: &str) {
        self.values.borrow_mut().insert(name.into(), value.into());
    }

    pub fn get(&self, name: &str) -> Option<String> {
        self.values.borrow().get(name).cloned()
    }

    pub fn stored_rules(&self) -> Option<Vec<Rule>> {
        self.rules.borrow().clone()
    }
}

impl RewriteOptions for MemoryRewriteOptions {
    fn get(&self, name: &str) -> Result<Option<String>, RewriteError> {
        Ok(MemoryRewriteOptions::get(self, name))
    }

    fn rules(&self) -> Result<Option<Vec<Rule>>, RewriteError> {
        Ok(self
            .rules
            .borrow()
            .clone()
            .filter(|rules| !rules.is_empty()))
    }

    fn set_rules(&self, rules: &[Rule]) -> Result<(), RewriteError> {
        *self.rules.borrow_mut() = Some(rules.to_vec());
        Ok(())
    }

    fn delete_rules(&self) -> Result<(), RewriteError> {
        *self.rules.borrow_mut() = None;
        Ok(())
    }

    fn update(&self, name: &str, value: &str) -> Result<(), RewriteError> {
        if name == "rewrite_rules" {
            *self.rules.borrow_mut() = None;
        }
        self.set(name, value);
        Ok(())
    }
}

/// State held in a cell; hooks see it as an opaque value.
pub struct MemoryRewriteStateSource {
    state: Rc<RefCell<RewriteState>>,
}

impl MemoryRewriteStateSource {
    pub fn new(state: RewriteState) -> MemoryRewriteStateSource {
        MemoryRewriteStateSource {
            state: Rc::new(RefCell::new(state)),
        }
    }

    /// The shared cell, for a Rust hook callback that changes the state as a plugin would change `$wp_rewrite`.
    pub fn cell(&self) -> Rc<RefCell<RewriteState>> {
        self.state.clone()
    }
}

impl RewriteStateSource for MemoryRewriteStateSource {
    fn load(&self) -> Result<RewriteState, RewriteError> {
        Ok(self.state.borrow().clone())
    }

    fn store(&self, state: &RewriteState) {
        *self.state.borrow_mut() = state.clone();
    }

    fn hook_value(&self) -> HookValue {
        HookValue::Opaque(self.state.clone())
    }
}

pub struct FixedUrls {
    pub home: String,
    pub site: String,
}

impl SiteUrls for FixedUrls {
    fn home_url(&self) -> String {
        self.home.clone()
    }

    fn site_url(&self) -> String {
        self.site.clone()
    }
}

/// A single-site install.
pub struct SingleSite;

impl Multisite for SingleSite {
    fn is_multisite_main_site(&self) -> bool {
        false
    }
}

pub struct FlagWpLoaded(Cell<bool>);

impl FlagWpLoaded {
    pub fn new(loaded: bool) -> FlagWpLoaded {
        FlagWpLoaded(Cell::new(loaded))
    }

    pub fn set(&self, loaded: bool) {
        self.0.set(loaded);
    }
}

impl WpLoaded for FlagWpLoaded {
    fn did_wp_loaded(&self) -> bool {
        self.0.get()
    }
}

#[derive(Default)]
pub struct MemoryPageUris {
    pub pages: Vec<(String, String)>,
    pub attachments: Vec<(String, String)>,
}

impl PageUris for MemoryPageUris {
    fn page_uris(&self) -> (UriIndex, UriIndex) {
        (self.pages.clone(), self.attachments.clone())
    }
}

#[derive(Default)]
pub struct MemoryPostTypes(pub Vec<(String, bool)>);

impl PostTypes for MemoryPostTypes {
    fn custom_post_types(&self) -> Vec<(String, bool)> {
        self.0.clone()
    }
}

#[derive(Default)]
pub struct MemoryQueryVars(RefCell<Vec<String>>);

impl MemoryQueryVars {
    pub fn added(&self) -> Vec<String> {
        self.0.borrow().clone()
    }
}

impl PublicQueryVars for MemoryQueryVars {
    fn add_query_var(&self, name: &str) {
        self.0.borrow_mut().push(name.into());
    }
}
