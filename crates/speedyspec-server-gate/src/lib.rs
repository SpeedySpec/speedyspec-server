//! The admin gate, per-app health and per-app maintenance, apart from any web framework. The server's adapters
//! implement [`calls::GateCalls`] over FastCGI; the presentation layer asks this crate what to answer.

pub mod calls;
pub mod health;
pub mod internal;
pub mod maintenance;

pub use calls::{BoxFuture, CallError, GateCalls};
pub use health::{HealthCounter, HealthMonitor, HealthSettings, HealthTargets, OnHealthChange};
pub use internal::is_internal;
pub use maintenance::{MaintenanceSet, MaintenanceSource};
