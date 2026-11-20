//! The Caching Store's errors.

use std::fmt;

/// A cache operation that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheError {
    /// The backend failed (a lost connection, a full disk).
    Backend { cache: String, message: String },
    /// `increment` or `decrement` met a value that is not an integer.
    NotANumber { group: String, key: String },
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CacheError::Backend { cache, message } => {
                write!(f, "cache `{cache}` failed: {message}")
            }
            CacheError::NotANumber { group, key } => {
                write!(
                    f,
                    "the cache entry `{key}` in group `{group}` is not an integer"
                )
            }
        }
    }
}

impl std::error::Error for CacheError {}
