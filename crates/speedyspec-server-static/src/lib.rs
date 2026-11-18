//! An in-memory, pre-compressed file set built from a static release, and serving from it.

mod build;

pub use build::{DEFAULT_LIMIT_BYTES, StaticBuildError, StaticFile, StaticSet};
