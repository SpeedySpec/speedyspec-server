use std::any::Any;
use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use speedyspec_wp_hooks::{HookApi, HookError};

use crate::{
    Multisite, PageUris, PostTypes, PublicQueryVars, RewriteOptions, RewriteStateSource, SiteUrls,
    WpLoaded,
};

/// Why a rewrite operation failed: an option or a hook callback failed.
///
/// `cause` is what a language bridge needs to rethrow the original failure unchanged (a PHP callback's exception
/// object), carried from [`HookError::payload`].
#[derive(Clone)]
pub struct RewriteError {
    pub message: String,
    pub cause: Option<Rc<dyn Any>>,
}

impl RewriteError {
    pub fn new(message: impl Into<String>) -> RewriteError {
        RewriteError {
            message: message.into(),
            cause: None,
        }
    }
}

impl fmt::Debug for RewriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RewriteError")
            .field("message", &self.message)
            .field("cause", &self.cause.is_some())
            .finish()
    }
}

/// Equal when the messages are: a cause is opaque.
impl PartialEq for RewriteError {
    fn eq(&self, other: &RewriteError) -> bool {
        self.message == other.message
    }
}

impl Eq for RewriteError {}

impl fmt::Display for RewriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rewrite: {}", self.message)
    }
}

impl std::error::Error for RewriteError {}

impl From<HookError> for RewriteError {
    fn from(err: HookError) -> RewriteError {
        RewriteError {
            message: err.message,
            cause: err.payload,
        }
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
