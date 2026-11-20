use std::time::Duration;

use speedyspec_firewall::{FirewallSettings, RawSettings};

fn raw() -> RawSettings {
    RawSettings {
        enabled: true,
        allow: "10.0.0.1".into(),
        block: "bad".into(),
        patterns: String::new(),
        rate_requests: 30,
        rate_window: 60,
        rate_ban: 300,
        login_max_failures: 5,
        login_window: 900,
        login_lockout: 600,
        signin_paths: "/wp-login.php\n\n/auth/login\n".into(),
        block_xmlrpc: true,
        block_user_enumeration: false,
    }
}

#[test]
fn settings_parse_all_fields_and_report_issues_by_setting() {
    let (settings, issues) = FirewallSettings::from_raw(&raw());
    assert!(settings.enabled);
    assert!(settings.allow.contains("10.0.0.1".parse().unwrap()));
    let rate = settings.rate.unwrap();
    assert_eq!(rate.requests, 30);
    assert_eq!(rate.window, Duration::from_secs(60));
    assert_eq!(rate.ban_for, Some(Duration::from_secs(300)));
    let login = settings.login.unwrap();
    assert_eq!(login.max_failures, 5);
    assert_eq!(login.lockout, Duration::from_secs(600));
    assert_eq!(login.signin_paths, vec!["/wp-login.php".to_string(), "/auth/login".to_string()]);
    assert!(settings.block_xmlrpc);
    assert!(!settings.block_user_enumeration);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].setting, "block");
}

#[test]
fn rate_disabled_when_requests_not_positive() {
    let mut raw = raw();
    raw.rate_requests = 0;
    assert!(FirewallSettings::from_raw(&raw).0.rate.is_none());
    let mut raw = self::raw();
    raw.rate_ban = 0;
    assert_eq!(FirewallSettings::from_raw(&raw).0.rate.unwrap().ban_for, None);
}

#[test]
fn login_disabled_when_max_failures_not_positive() {
    let mut raw = raw();
    raw.login_max_failures = -1;
    assert!(FirewallSettings::from_raw(&raw).0.login.is_none());
}

#[test]
fn raw_settings_defaults_match_the_option_table() {
    let raw = RawSettings::default();
    assert!(!raw.enabled);
    assert_eq!(raw.rate_requests, 0);
    assert_eq!(raw.rate_window, 60);
    assert_eq!(raw.rate_ban, 0);
    assert_eq!(raw.login_max_failures, 5);
    assert_eq!(raw.login_window, 900);
    assert_eq!(raw.login_lockout, 900);
    assert_eq!(raw.signin_paths, "/wp-login.php");
    assert!(!raw.block_xmlrpc);
    assert!(!raw.block_user_enumeration);
}
