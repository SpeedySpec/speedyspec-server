//! Repository and Unit of Work contracts.

use std::fmt::{self, Debug};
use std::hash::Hash;

use crate::{DbError, EngineHandle};

/// An entity with an identity; it knows nothing of how it is stored.
pub trait Identified {
    type Id: Clone + Eq + Hash + Debug + 'static;
    fn identity(&self) -> Self::Id;
}

/// Stores and finds one aggregate; aggregate repositories add their own methods beside these.
pub trait Repository<E: Identified> {
    fn find(&self, id: &E::Id) -> Result<Option<E>, DbError>;
    fn add(&self, entity: E) -> Result<(), DbError>;
    fn update(&self, entity: E) -> Result<(), DbError>;
    fn remove(&self, id: &E::Id) -> Result<(), DbError>;
}

/// The kind of a pending change; a unit of work applies new, then dirty, then removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    New,
    Dirty,
    Removed,
}

/// One change waiting for commit, on one engine.
pub trait PendingChange {
    fn engine(&self) -> &str;
    fn kind(&self) -> ChangeKind;
    fn apply(&self, handle: &EngineHandle) -> Result<(), DbError>;
}

/// The engines that committed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitReport {
    pub committed: Vec<String>,
}

/// A commit that failed before any engine committed, or after some did.
#[derive(Debug, Clone, PartialEq)]
pub enum CommitError {
    Failed {
        engine: String,
        error: DbError,
    },
    Partial {
        committed: Vec<String>,
        failed: String,
        error: DbError,
    },
}

impl fmt::Display for CommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommitError::Failed { engine, error } => {
                write!(f, "commit failed on engine `{engine}`: {error}")
            }
            CommitError::Partial {
                committed,
                failed,
                error,
            } => write!(
                f,
                "commit partly applied: committed on {}, failed on `{failed}`: {error}",
                committed.join(", ")
            ),
        }
    }
}

impl std::error::Error for CommitError {}

/// Collects changes and commits them together, one transaction per engine.
pub trait UnitOfWork {
    fn register(&self, change: Box<dyn PendingChange>);
    fn after_commit(&self, callback: Box<dyn FnOnce()>);
    fn commit(&self) -> Result<CommitReport, CommitError>;
    fn discard(&self);
    fn is_empty(&self) -> bool;
}
