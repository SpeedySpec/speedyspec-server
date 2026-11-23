//! What the scheduling core returns and how it fails.

use std::any::Any;
use std::fmt;
use std::rc::Rc;

use speedyspec_hooks::Value;

/// The option's stored bytes as read, the version an Optimistic Offline Lock compares; `None` when the option is
/// absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version(pub Option<Vec<u8>>);

/// What a run of due events did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunSummary {
    /// Events this run claimed and ran.
    pub ran: u64,
    /// Due events another runner claimed first: skipped, with no hook fired.
    pub claimed_by_others: u64,
    /// Due events this runner could not claim (its write kept conflicting): not run, left for the next call.
    pub unclaimed: u64,
    /// When the next event is due (the current time when the run stopped at its budget); `None` with none left.
    pub next: Option<i64>,
    /// Sites visited: 1 on a single site, each site whose events were run on a network.
    pub sites: u64,
}

/// What a compare-and-swap save did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOutcome {
    /// Saved.
    Written,
    /// `update_option`'s `false`: nothing changed, or a filter refused.
    Unchanged,
    /// Another process wrote first.
    Conflict,
}

/// A scheduling function's answer: a value for PHP, or a `WP_Error` to build (code and translated message).
#[derive(Debug, Clone, PartialEq)]
pub enum CronResult {
    Value(Value),
    Error {
        code: &'static str,
        message: String,
    },
    /// `wp_clear_scheduled_hook()`'s merged error: a new `WP_Error` that `merge_from`s each entry (an `Error`, or a
    /// `Value` holding a `WP_Error` a filter returned).
    Merge(Vec<CronResult>),
}

/// A failure WordPress would not hide: a hook callback threw, or storage failed.
pub struct CronError {
    pub message: String,
    /// A PHP exception or other payload from a hook, carried through unchanged.
    pub payload: Option<Rc<dyn Any>>,
}

impl CronError {
    pub fn new(message: impl Into<String>) -> CronError {
        CronError {
            message: message.into(),
            payload: None,
        }
    }
}

impl fmt::Debug for CronError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CronError")
            .field("message", &self.message)
            .field("payload", &self.payload.is_some())
            .finish()
    }
}

impl fmt::Display for CronError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CronError {}
