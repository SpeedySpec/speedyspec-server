//! The firewall's domain: what it knows about a request, the rules' building blocks, its settings, its verdicts
//! and the ports it needs from the outside (counters, bans, settings, a clock and a log).
//!
//! Nothing here does I/O. Adapters live in `speedyspec-firewall-infra-*`; the rules live in
//! `speedyspec-firewall-app`.

mod facts;
mod ip_set;
mod pattern;
mod ports;
mod settings;
mod verdict;

pub use facts::RequestFacts;
pub use ip_set::IpSet;
pub use pattern::{Field, PatternRule};
pub use ports::{BanStore, Clock, CounterStore, FirewallError, FirewallLog, SettingsSource};
pub use settings::{FirewallSettings, LoginPolicy, RateLimit, RawSettings, SettingsIssue};
pub use verdict::Verdict;
