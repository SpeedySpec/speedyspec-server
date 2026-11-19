//! Value types and contracts for the SpeedySpec server. This crate does no I/O; implementations live in other crates.

pub mod access;
pub mod app;
pub mod cluster;
pub mod contracts;
pub mod error;
pub mod manifest;
pub mod mount;
pub mod release;
pub mod state;

pub use access::{AccessCheck, AccessDecision, AccessRequest, Identity};
pub use app::{AppId, AppKind};
pub use cluster::{ClusterMembers, LeaderView, Leadership, LeaseTimings, MemberReport, PodServer};
pub use contracts::{
    AppRuntime, BinaryInspector, Prepared, ReleaseSource, ReleaseStore, SignatureVerifier,
    SourceError, StateError, StateStore, StoreError, VerifyError,
};
pub use error::DomainError;
pub use manifest::{
    CacheRule, HealthSection, MANIFEST_FILE, PhpSection, ReleaseManifest, StaticSection,
};
pub use mount::{Access, HostMatch, Mount, MountTable};
pub use release::{Artifact, IndexEntry, ReleaseIndex};
pub use state::{AppState, PendingGroup, ReleaseState, Rollout, ServerState, StagedSet};
