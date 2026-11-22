//! What the scheduling core returns and how it fails.

use std::any::Any;
use std::fmt;
use std::rc::Rc;

use speedyspec_hooks::Value;

/// The option's stored bytes as read, the version an Optimistic Offline Lock compares; `None` when the option is
/// absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version(pub Option<Vec<u8>>);

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
    Error { code: &'static str, message: String },
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
