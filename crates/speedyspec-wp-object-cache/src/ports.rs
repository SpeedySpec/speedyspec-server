//! The object cache's outbound ports, with their Rust adapters.

use std::rc::Rc;

use speedyspec_hooks::Value;

/// `is_multisite()` and `get_current_blog_id()`.
pub trait SiteContext {
    fn is_multisite(&self) -> bool;
    fn current_blog_id(&self) -> i64;
}

/// A single site, blog 1.
pub struct SingleSite;

impl SiteContext for SingleSite {
    fn is_multisite(&self) -> bool {
        false
    }
    fn current_blog_id(&self) -> i64 {
        1
    }
}

/// `_doing_it_wrong()`, `_deprecated_function()`, and a value the second level could not keep.
pub trait CacheNotices {
    fn doing_it_wrong(&self, function: &str, message: &str, version: &str);
    fn deprecated_function(&self, function: &str, version: &str, replacement: &str);
    fn persistence_miss(&self, group: &str, key: &str, why: &str);
}

/// Notices written to the worker's log.
pub struct LoggedNotices;

impl CacheNotices for LoggedNotices {
    fn doing_it_wrong(&self, function: &str, message: &str, version: &str) {
        eprintln!("speedyspec_wordpress: {function} was called incorrectly: {message} ({version})");
    }
    fn deprecated_function(&self, function: &str, version: &str, replacement: &str) {
        eprintln!(
            "speedyspec_wordpress: {function} is deprecated since {version}; use {replacement}"
        );
    }
    fn persistence_miss(&self, group: &str, key: &str, why: &str) {
        eprintln!(
            "speedyspec_wordpress: the object cache kept {group}/{key} in the request only: {why}"
        );
    }
}

/// `wp_suspend_cache_addition()`.
pub trait AdditionSuspension {
    fn suspended(&self) -> bool;
}

/// Cache addition is never suspended.
pub struct NeverSuspended;

impl AdditionSuspension for NeverSuspended {
    fn suspended(&self) -> bool {
        false
    }
}

/// Why a value could not be encoded or decoded for the second level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// The value has no serialized form here (a closure, a resource).
    Refused(String),
    /// The stored bytes do not decode.
    Corrupt(String),
}

/// How first-level values become second-level bytes (Strategy).
pub trait ValueCodec {
    fn encode(&self, value: &Value) -> Result<Vec<u8>, CodecError>;
    fn decode(&self, bytes: &[u8]) -> Result<Value, CodecError>;
}

/// The ports an object cache needs.
#[derive(Clone)]
pub struct ObjectCachePorts {
    pub site: Rc<dyn SiteContext>,
    pub notices: Rc<dyn CacheNotices>,
    pub suspension: Rc<dyn AdditionSuspension>,
    pub codec: Rc<dyn ValueCodec>,
}

impl ObjectCachePorts {
    /// The Rust adapters: a single site, logged notices, never suspended, PHP's serialize format.
    pub fn rust() -> ObjectCachePorts {
        ObjectCachePorts {
            site: Rc::new(SingleSite),
            notices: Rc::new(LoggedNotices),
            suspension: Rc::new(NeverSuspended),
            codec: Rc::new(crate::codec::SerializeCodec),
        }
    }
}
