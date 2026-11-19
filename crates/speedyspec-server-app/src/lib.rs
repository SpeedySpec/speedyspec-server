//! Use cases of the SpeedySpec server. Depends only on the domain crate.

pub mod cluster;
pub mod compat;
pub mod download;
pub mod releases;
pub mod rollout;

pub use cluster::{Clock, Cluster, Liveness, STALE_AFTER};
pub use compat::{Incompatibility, check_set};
pub use releases::{
    AppPolicy, BootReport, ClusterStatus, FollowOutcome, MaintenanceSink, MemberStatus, Migrator,
    ReleaseError, ReleaseManager, SERVER_BINARY, ServerUpgrade, Signing, TakeOver, confirm_server,
    register_running_binary,
};
pub use rollout::{RolloutStep, leader_last, next_step};
