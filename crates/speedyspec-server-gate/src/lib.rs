//! The admin gate, per-app health and per-app maintenance, apart from any web framework. The server's adapters
//! implement [`calls::GateCalls`] over FastCGI; the presentation layer asks this crate what to answer.

pub mod calls;
pub mod gate;
pub mod health;
pub mod internal;
pub mod maintenance;
pub mod rules;
pub mod session;

pub use calls::{BoxFuture, CallError, GateCalls, SessionAnswer, SiteState};
pub use gate::{GATE_VERSION, GateAccess, GateSettings, GateView, cookie_header};
pub use health::{HealthCounter, HealthMonitor, HealthSettings, HealthTargets, OnHealthChange};
pub use internal::is_internal;
pub use maintenance::{MaintenanceSet, MaintenanceSource};
pub use rules::{AccessRules, RuleOutcome};
pub use session::{CacheOutcome, SessionCache};
