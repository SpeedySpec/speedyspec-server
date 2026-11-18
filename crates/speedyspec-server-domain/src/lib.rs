//! Value types and contracts for the SpeedySpec server. This crate does no I/O; implementations live in other crates.

pub mod access;
pub mod app;
pub mod error;
pub mod manifest;
pub mod mount;

pub use access::{AccessCheck, AccessDecision, AccessRequest, Identity};
pub use app::{AppId, AppKind};
pub use error::DomainError;
pub use manifest::{CacheRule, MANIFEST_FILE, ReleaseManifest, StaticSection};
pub use mount::{Access, HostMatch, Mount, MountTable};
