//! What the metadata API reports when it cannot do as WordPress does.

use std::fmt;

#[derive(Debug)]
pub enum MetaError {
    Db(String),
    /// A meta type with no table.
    NoTable(String),
}

impl fmt::Display for MetaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MetaError::Db(e) => write!(f, "the database failed: {e}"),
            MetaError::NoTable(t) => write!(f, "the meta type {t} has no table"),
        }
    }
}

impl std::error::Error for MetaError {}
