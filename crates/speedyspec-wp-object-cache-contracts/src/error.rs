//! The object cache's errors (what PHP throws for them).

use std::fmt;

/// A key or group PHP cannot use as an array key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectCacheError {
    /// PHP's `TypeError: Illegal offset type` (an array or object as a key); holds the type's name.
    IllegalOffset(String),
    /// A key the Rust side cannot hold (a string that is not UTF-8).
    Unsupported(String),
}

impl fmt::Display for ObjectCacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectCacheError::IllegalOffset(t) => {
                write!(f, "Cannot access offset of type {t} on array")
            }
            ObjectCacheError::Unsupported(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ObjectCacheError {}
