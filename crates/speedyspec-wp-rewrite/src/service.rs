use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use speedyspec_wp_hooks::{HookApi, HookError};

use crate::{
    Multisite, PageUris, PostTypes, PublicQueryVars, RewriteOptions, RewriteStateSource, SiteUrls,
    WpLoaded,
};

/// Why a rewrite operation failed: an option or a hook callback failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteError(pub String);

impl fmt::Display for RewriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rewrite: {}", self.0)
    }
}

impl std::error::Error for RewriteError {}

impl From<HookError> for RewriteError {
    fn from(err: HookError) -> RewriteError {
        RewriteError(err.message)
    }
}

/// What the rewrite service needs from the rest of WordPress.
#[derive(Clone)]
pub struct RewriteDeps {
    pub options: Rc<dyn RewriteOptions>,
    pub urls: Rc<dyn SiteUrls>,
    pub multisite: Rc<dyn Multisite>,
    pub loaded: Rc<dyn WpLoaded>,
    pub pages: Rc<dyn PageUris>,
    pub post_types: Rc<dyn PostTypes>,
    pub query_vars: Rc<dyn PublicQueryVars>,
    pub hooks: Rc<HookApi>,
    /// The canonical state: in PHP, the `$wp_rewrite` global. Work deferred to `wp_loaded` uses it.
    pub state: Rc<dyn RewriteStateSource>,
}

/// `WP_Rewrite`'s methods, each over a [`crate::RewriteState`] the caller holds.
pub struct RewriteService {
    pub(crate) deps: RewriteDeps,
    /// `flush_rules()`'s `static $do_hard_later`: a hard flush asked for before `wp_loaded`.
    pub(crate) do_hard_later: Rc<Cell<Option<bool>>>,
}

impl RewriteService {
    pub fn new(deps: RewriteDeps) -> RewriteService {
        RewriteService {
            deps,
            do_hard_later: Rc::new(Cell::new(None)),
        }
    }

    pub fn deps(&self) -> &RewriteDeps {
        &self.deps
    }
}
