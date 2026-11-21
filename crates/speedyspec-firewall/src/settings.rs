use std::time::Duration;

use crate::ip_set::IpSet;
use crate::pattern::PatternRule;

/// A settings line the firewall skipped, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsIssue {
    /// The setting's name without the `speedyspec_firewall_` prefix.
    pub setting: &'static str,
    /// Counted from 1.
    pub line: usize,
    pub message: String,
}

/// The settings as stored: one field per `speedyspec_firewall_*` option, named without the prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSettings {
    pub enabled: bool,
    pub allow: String,
    pub block: String,
    pub patterns: String,
    pub rate_requests: i64,
    pub rate_window: i64,
    pub rate_ban: i64,
    pub login_max_failures: i64,
    pub login_window: i64,
    pub login_lockout: i64,
    pub signin_paths: String,
    pub block_xmlrpc: bool,
    pub block_user_enumeration: bool,
}

impl Default for RawSettings {
    fn default() -> RawSettings {
        RawSettings {
            enabled: false,
            allow: String::new(),
            block: String::new(),
            patterns: String::new(),
            rate_requests: 0,
            rate_window: 60,
            rate_ban: 0,
            login_max_failures: 5,
            login_window: 900,
            login_lockout: 900,
            signin_paths: "/wp-login.php".into(),
            block_xmlrpc: false,
            block_user_enumeration: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    pub requests: u64,
    pub window: Duration,
    pub ban_for: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginPolicy {
    pub max_failures: u64,
    pub window: Duration,
    pub lockout: Duration,
    pub signin_paths: Vec<String>,
}

/// The settings, parsed and ready for the rules.
#[derive(Debug, Clone)]
pub struct FirewallSettings {
    pub enabled: bool,
    pub allow: IpSet,
    pub block: IpSet,
    pub patterns: Vec<PatternRule>,
    pub rate: Option<RateLimit>,
    pub login: Option<LoginPolicy>,
    pub block_xmlrpc: bool,
    pub block_user_enumeration: bool,
}

impl FirewallSettings {
    pub fn from_raw(raw: &RawSettings) -> (FirewallSettings, Vec<SettingsIssue>) {
        let mut issues = Vec::new();
        let (allow, allow_issues) = IpSet::parse(&raw.allow);
        issues.extend(allow_issues.into_iter().map(|issue| SettingsIssue {
            setting: "allow",
            ..issue
        }));
        let (block, block_issues) = IpSet::parse(&raw.block);
        issues.extend(block_issues.into_iter().map(|issue| SettingsIssue {
            setting: "block",
            ..issue
        }));
        let (patterns, pattern_issues) = PatternRule::parse_lines(&raw.patterns);
        issues.extend(pattern_issues);

        let rate = (raw.rate_requests > 0).then(|| RateLimit {
            requests: raw.rate_requests as u64,
            window: seconds(raw.rate_window),
            ban_for: (raw.rate_ban > 0).then(|| seconds(raw.rate_ban)),
        });
        let login = (raw.login_max_failures > 0).then(|| LoginPolicy {
            max_failures: raw.login_max_failures as u64,
            window: seconds(raw.login_window),
            lockout: seconds(raw.login_lockout),
            signin_paths: raw
                .signin_paths
                .lines()
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(String::from)
                .collect(),
        });
        let settings = FirewallSettings {
            enabled: raw.enabled,
            allow,
            block,
            patterns,
            rate,
            login,
            block_xmlrpc: raw.block_xmlrpc,
            block_user_enumeration: raw.block_user_enumeration,
        };
        (settings, issues)
    }
}

/// The longest window, ban or lockout: a typo must never ban a client for good.
const MAX_SECONDS: i64 = 30 * 24 * 60 * 60;

/// A window or ban length between one second and thirty days.
fn seconds(value: i64) -> Duration {
    Duration::from_secs(value.clamp(1, MAX_SECONDS) as u64)
}
