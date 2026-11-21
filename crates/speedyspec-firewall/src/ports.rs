use std::fmt;
use std::time::{Duration, SystemTime};

use crate::settings::RawSettings;

/// Any failure the firewall's ports report. The firewall never blocks because of one: it lets the request through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirewallError(pub String);

impl fmt::Display for FirewallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FirewallError {}

/// Counts hits in fixed windows.
pub trait CounterStore {
    /// Counts one hit for `key` in the fixed `window` that contains now, and returns the count so far.
    fn hit(&self, key: &str, window: Duration) -> Result<u64, FirewallError>;
}

/// Bans that expire on their own.
pub trait BanStore {
    fn ban(&self, key: &str, for_: Duration, reason: &str) -> Result<(), FirewallError>;
    /// The reason `key` is banned, or `None` when it is not.
    fn banned(&self, key: &str) -> Result<Option<String>, FirewallError>;
    fn lift(&self, key: &str) -> Result<(), FirewallError>;
}

/// Where the firewall's settings come from.
pub trait SettingsSource {
    fn load(&self) -> Result<RawSettings, FirewallError>;
}

pub trait Clock {
    fn now(&self) -> SystemTime;
}

/// Where the firewall reports what it could not do.
pub trait FirewallLog {
    fn warn(&self, message: &str);
}
