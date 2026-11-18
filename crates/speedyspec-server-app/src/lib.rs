//! Use cases of the SpeedySpec server. Depends only on the domain crate.

pub mod compat;
pub mod download;
pub mod releases;

pub use compat::{Incompatibility, check_set};
pub use releases::{AppPolicy, BootReport, ReleaseError, ReleaseManager, Signing};
