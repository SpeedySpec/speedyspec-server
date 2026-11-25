//! What the metadata API reports when it cannot do as WordPress does.

use std::fmt;

/// A failure the metadata API cannot hide, as `$wpdb` hides none of these.
#[derive(Debug)]
pub enum MetaError {
    /// The database reported a failure.
    Db(String),
}

impl fmt::Display for MetaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MetaError::Db(e) => write!(f, "the database failed: {e}"),
        }
    }
}

impl std::error::Error for MetaError {}
