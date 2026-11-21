mod doubles;

use std::rc::Rc;
use std::time::Duration;

use doubles::*;
use speedyspec_firewall::{BanStore, FirewallSettings, RawSettings, RequestFacts, Verdict};
use speedyspec_firewall_app::{Firewall, FirewallCache, FirewallScope};

fn raw() -> RawSettings {
    RawSettings {
        enabled: true,
        ..RawSettings::default()
    }
}

struct World {
    counters: Rc<MemoryCounters>,
    bans: Rc<MemoryBans>,
    log: Rc<RecordingLog>,
}

impl World {
    fn new() -> World {
        World {
            counters: Rc::default(),
            bans: Rc::default(),
            log: Rc::default(),
        }
    }
    fn firewall(&self, raw: &RawSettings) -> Firewall {
        let (settings, issues) = FirewallSettings::from_raw(raw);
        assert!(issues.is_empty(), "{issues:?}");
        Firewall::from_settings(
            &settings,
            FirewallScope::Public,
            self.counters.clone(),
            self.bans.clone(),
            self.log.clone(),
        )
    }
}

fn status(verdict: &Verdict) -> Option<u16> {
    match verdict {
        Verdict::Block { status, .. } => Some(*status),
        _ => None,
    }
}

#[test]
fn blocks_listed_ip_with_403_and_no_store_reason() {
    let world = World::new();
    let firewall = world.firewall(&RawSettings {
        block: "192.0.2.0/24".into(),
        ..raw()
    });
    let verdict = firewall.check(&facts("192.0.2.7", "GET", "/"));
    assert_eq!(status(&verdict), Some(403));
    assert_eq!(
        firewall.check(&facts("198.51.100.1", "GET", "/")),
        Verdict::Continue
    );
}

#[test]
fn allow_list_wins_over_everything() {
    let world = World::new();
    world
        .bans
        .ban("ip:192.0.2.7", Duration::from_secs(60), "earlier")
        .unwrap();
    let firewall = world.firewall(&RawSettings {
        allow: "192.0.2.7".into(),
        block: "192.0.2.7".into(),
        rate_requests: 1,
        ..raw()
    });
    for _ in 0..3 {
        assert_eq!(
            firewall.check(&facts("192.0.2.7", "GET", "/")),
            Verdict::Allow
        );
    }
    assert!(world.counters.counts.borrow().is_empty());
}

#[test]
fn rate_limit_blocks_after_limit_and_bans() {
    let world = World::new();
    let firewall = world.firewall(&RawSettings {
        rate_requests: 2,
        rate_window: 60,
        rate_ban: 300,
        ..raw()
    });
    let request = facts("192.0.2.7", "GET", "/");
    assert_eq!(firewall.check(&request), Verdict::Continue);
    assert_eq!(firewall.check(&request), Verdict::Continue);
    match firewall.check(&request) {
        Verdict::Block {
            status,
            retry_after,
            ..
        } => {
            assert_eq!(status, 429);
            assert_eq!(retry_after, Some(Duration::from_secs(300)));
        }
        other => panic!("expected a block, got {other:?}"),
    }
    assert!(world.bans.banned("ip:192.0.2.7").unwrap().is_some());
    assert_eq!(status(&firewall.check(&request)), Some(403));
}

#[test]
fn rate_limit_without_ban_retries_after_window() {
    let world = World::new();
    let firewall = world.firewall(&RawSettings {
        rate_requests: 1,
        rate_window: 45,
        ..raw()
    });
    let request = facts("192.0.2.7", "GET", "/");
    firewall.check(&request);
    match firewall.check(&request) {
        Verdict::Block {
            status: 429,
            retry_after,
            ..
        } => assert_eq!(retry_after, Some(Duration::from_secs(45))),
        other => panic!("expected a 429, got {other:?}"),
    }
    assert!(world.bans.bans.borrow().is_empty());
}

#[test]
fn pattern_rule_blocks_with_its_status() {
    let world = World::new();
    let firewall = world.firewall(&RawSettings {
        patterns: "path 429 ^/slow\npath 403 ^/wp-config".into(),
        ..raw()
    });
    assert_eq!(
        status(&firewall.check(&facts("192.0.2.7", "GET", "/wp-config.php"))),
        Some(403)
    );
    match firewall.check(&facts("192.0.2.7", "GET", "/slow")) {
        Verdict::Block {
            status: 429,
            retry_after: None,
            ..
        } => {}
        other => panic!("expected a 429 without retry, got {other:?}"),
    }
}

#[test]
fn xmlrpc_blocked_only_when_enabled() {
    let world = World::new();
    let off = world.firewall(&raw());
    assert_eq!(
        off.check(&facts("192.0.2.7", "POST", "/xmlrpc.php")),
        Verdict::Continue
    );
    let on = world.firewall(&RawSettings {
        block_xmlrpc: true,
        ..raw()
    });
    assert_eq!(
        status(&on.check(&facts("192.0.2.7", "POST", "/xmlrpc.php"))),
        Some(403)
    );
}

#[test]
fn user_enumeration_blocked_for_anonymous_only() {
    let world = World::new();
    let firewall = world.firewall(&RawSettings {
        block_user_enumeration: true,
        ..raw()
    });
    let check = |uri: &str, extra: &[(&str, &str)]| {
        let mut params = params("192.0.2.7", "GET", uri, None);
        params.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        status(&firewall.check(&RequestFacts::from_params(&params).unwrap()))
    };
    let cookie = ("HTTP_COOKIE", "wordpress_logged_in_x=1");
    assert_eq!(check("/?author=2", &[]), Some(403));
    assert_eq!(check("/?p=1&author=12", &[]), Some(403));
    assert_eq!(check("/?author=1%20", &[]), Some(403));
    assert_eq!(check("/wp-json/wp/v2/users", &[]), Some(403));
    assert_eq!(check("/wp-json/wp/v2/users/1", &[]), Some(403));
    assert_eq!(check("/?rest_route=/wp/v2/users", &[]), Some(403));
    assert_eq!(check("/?rest_route=%2Fwp%2Fv2%2Fusers%2F1", &[]), Some(403));
    assert_eq!(check("/?author=bob", &[]), None);
    assert_eq!(check("/wp-json/wp/v2/usersx", &[]), None);
    assert_eq!(check("/?rest_route=/wp/v2/posts", &[]), None);
    // Forged credentials do not bypass the rule.
    assert_eq!(
        check(
            "/wp-json/wp/v2/users",
            &[("HTTP_AUTHORIZATION", "Bearer x")]
        ),
        Some(403)
    );
    assert_eq!(check("/wp-json/wp/v2/users", &[cookie]), Some(403));
    assert_eq!(check("/?author=1", &[cookie]), Some(403));
    // Real REST credentials (application passwords, or the editor's cookie and nonce) and admin screens pass.
    assert_eq!(
        check(
            "/wp-json/wp/v2/users",
            &[("HTTP_AUTHORIZATION", "Basic eA==")]
        ),
        None
    );
    assert_eq!(
        check(
            "/wp-json/wp/v2/users/me",
            &[cookie, ("HTTP_X_WP_NONCE", "n")]
        ),
        None
    );
    assert_eq!(check("/wp-admin/edit.php?author=1", &[cookie]), None);
}

#[test]
fn store_errors_fail_open() {
    let log = Rc::new(RecordingLog::default());
    let failing = Rc::new(FailingStores);
    let (settings, _) = FirewallSettings::from_raw(&RawSettings {
        rate_requests: 1,
        ..raw()
    });
    let firewall = Firewall::from_settings(
        &settings,
        FirewallScope::Public,
        failing.clone(),
        failing,
        log.clone(),
    );
    assert_eq!(
        firewall.check(&facts("192.0.2.7", "GET", "/")),
        Verdict::Continue
    );
    let messages = log.messages.borrow();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages.iter().any(|m| m.starts_with("ban: ")));
    assert!(messages.iter().any(|m| m.starts_with("rate-limit: ")));
}

#[test]
fn disabled_settings_continue_everything() {
    let world = World::new();
    let firewall = world.firewall(&RawSettings {
        enabled: false,
        block: "0.0.0.0/0".into(),
        ..raw()
    });
    assert_eq!(
        firewall.check(&facts("192.0.2.7", "GET", "/")),
        Verdict::Continue
    );
}

#[test]
fn cache_rebuilds_only_when_raw_settings_change() {
    let world = World::new();
    let source = Rc::new(FixedSettings {
        raw: std::cell::RefCell::new(Ok(RawSettings {
            block: "bad".into(),
            ..raw()
        })),
    });
    let cache = FirewallCache::new(
        source.clone(),
        FirewallScope::Public,
        world.counters.clone(),
        world.bans.clone(),
        world.log.clone(),
    );
    assert!(cache.current().is_some());
    assert!(cache.current().is_some());
    assert_eq!(world.log.messages.borrow().len(), 1);
    *source.raw.borrow_mut() = Ok(RawSettings {
        block: "worse".into(),
        ..raw()
    });
    assert!(cache.current().is_some());
    assert_eq!(world.log.messages.borrow().len(), 2);
}

#[test]
fn cache_load_error_gives_none_and_logs() {
    let world = World::new();
    let source = Rc::new(FixedSettings {
        raw: std::cell::RefCell::new(Err(speedyspec_firewall::FirewallError("db down".into()))),
    });
    let cache = FirewallCache::new(
        source,
        FirewallScope::Public,
        world.counters.clone(),
        world.bans.clone(),
        world.log.clone(),
    );
    assert!(cache.current().is_none());
    assert!(cache.login_guard().is_none());
    assert_eq!(world.log.messages.borrow().len(), 2);
}

#[test]
fn cache_login_guard_follows_settings() {
    let world = World::new();
    let source = Rc::new(FixedSettings {
        raw: std::cell::RefCell::new(Ok(RawSettings {
            enabled: false,
            ..raw()
        })),
    });
    let cache = FirewallCache::new(
        source.clone(),
        FirewallScope::Public,
        world.counters.clone(),
        world.bans.clone(),
        world.log.clone(),
    );
    assert!(cache.login_guard().is_none());
    *source.raw.borrow_mut() = Ok(raw());
    assert!(cache.login_guard().is_some());
    assert!(cache.settings().unwrap().enabled);
}

#[test]
fn ipv6_clients_share_counters_and_bans_per_64() {
    let world = World::new();
    let firewall = world.firewall(&RawSettings {
        rate_requests: 2,
        rate_ban: 300,
        ..raw()
    });
    assert_eq!(
        firewall.check(&facts("2001:db8:1:2::1", "GET", "/")),
        Verdict::Continue
    );
    assert_eq!(
        firewall.check(&facts("2001:db8:1:2::2", "GET", "/")),
        Verdict::Continue
    );
    assert_eq!(
        status(&firewall.check(&facts("2001:db8:1:2::3", "GET", "/"))),
        Some(429)
    );
    assert_eq!(
        status(&firewall.check(&facts("2001:db8:1:2:ffff::9", "GET", "/"))),
        Some(403)
    );
    assert_eq!(
        firewall.check(&facts("2001:db8:1:3::1", "GET", "/")),
        Verdict::Continue
    );
}

#[test]
fn admin_scope_never_rate_limits_or_bans() {
    let world = World::new();
    world
        .bans
        .ban("ip:192.0.2.7", Duration::from_secs(60), "rate limit")
        .unwrap();
    let raw = RawSettings {
        rate_requests: 1,
        rate_ban: 300,
        block: "198.51.100.0/24".into(),
        ..raw()
    };
    let (settings, _) = FirewallSettings::from_raw(&raw);
    let admin = Firewall::from_settings(
        &settings,
        FirewallScope::Admin,
        world.counters.clone(),
        world.bans.clone(),
        world.log.clone(),
    );
    for _ in 0..5 {
        assert_eq!(
            admin.check(&facts("192.0.2.7", "GET", "/wp-json/wp-admin/v1/posts")),
            Verdict::Continue
        );
    }
    assert!(world.counters.counts.borrow().is_empty());
    assert_eq!(
        status(&admin.check(&facts("198.51.100.4", "GET", "/"))),
        Some(403)
    );
    world
        .bans
        .ban("login:192.0.2.7", Duration::from_secs(60), "login failures")
        .unwrap();
    assert_eq!(
        status(&admin.check(&facts("192.0.2.7", "POST", "/wp-login.php"))),
        Some(429)
    );
}
