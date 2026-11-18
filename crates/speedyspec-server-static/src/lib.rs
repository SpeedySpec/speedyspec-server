//! An in-memory, pre-compressed file set built from a static release, and serving from it.

mod build;
pub mod encoding;
pub mod range;
mod respond;

pub use build::{DEFAULT_LIMIT_BYTES, StaticBuildError, StaticFile, StaticSet};
