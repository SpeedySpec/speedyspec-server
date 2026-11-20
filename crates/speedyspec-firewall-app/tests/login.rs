mod doubles;

use std::rc::Rc;
use std::time::Duration;

use doubles::*;
use speedyspec_firewall::{BanStore, FirewallSettings, RawSettings, Verdict};
use speedyspec_firewall_app::{Firewall, LoginGuard};

#[test]
fn login_guard_locks_after_max_failures() {
    let counters = Rc::new(MemoryCounters::default());
    let bans = Rc::new(MemoryBans::default());
    let log = Rc::new(RecordingLog::default());
    let raw = RawSettings { enabled: true, login_max_failures: 3, login_lockout: 600, ..RawSettings::default() };
    let (settings, _) = FirewallSettings::from_raw(&raw);
    let guard = LoginGuard::new(settings.login.clone().unwrap(), counters.clone(), bans.clone());
    let client = "192.0.2.9".parse().unwrap();

    guard.record_failure(client).unwrap();
    guard.record_failure(client).unwrap();
    assert!(bans.banned("login:192.0.2.9").unwrap().is_none());
    guard.record_failure(client).unwrap();
    assert!(bans.banned("login:192.0.2.9").unwrap().is_some());
    assert_eq!(bans.bans.borrow()["login:192.0.2.9"].0, Duration::from_secs(600));

    let firewall = Firewall::from_settings(&settings, counters, bans, log);
    match firewall.check(&facts("192.0.2.9", "POST", "/wp-login.php")) {
        Verdict::Block { status: 429, retry_after, .. } => assert_eq!(retry_after, Some(Duration::from_secs(600))),
        other => panic!("expected a 429, got {other:?}"),
    }
    assert_eq!(firewall.check(&facts("192.0.2.9", "GET", "/wp-login.php")), Verdict::Continue);
    assert_eq!(firewall.check(&facts("192.0.2.10", "POST", "/wp-login.php")), Verdict::Continue);
}
