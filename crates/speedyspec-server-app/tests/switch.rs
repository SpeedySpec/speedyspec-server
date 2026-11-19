mod support;

use std::sync::Arc;
use std::time::Duration;

use speedyspec_server_app::ReleaseError;
use speedyspec_server_domain::AppKind;
use support::{SERVER_VERSION, World, id, v};

/// `blog` with releases 1.0.0..=1.3.0 published, keep = `keep`.
fn world(keep: u32) -> World {
    let mut w = World::new();
    w.app("blog", AppKind::Php, keep);
    for version in ["1.0.0", "1.1.0", "1.2.0", "1.3.0"] {
        w.publish("blog", version, "");
    }
    w
}

fn install_and_switch(manager: &speedyspec_server_app::ReleaseManager, version: &str) {
    manager.install(&id("blog"), Some(v(version))).unwrap();
    manager.switch(&[(id("blog"), v(version))]).unwrap();
}

#[test]
fn switch_activates_and_records_previous() {
    let w = world(3);
    let manager = w.manager();
    install_and_switch(&manager, "1.0.0");
    let revision = w.saved().revision;
    install_and_switch(&manager, "1.1.0");
    assert_eq!(
        w.runtime.events(),
        [
            "prepare blog 1.0.0",
            "activate blog 1.0.0",
            "prepare blog 1.1.0",
            "activate blog 1.1.0",
            "retire blog 1.0.0"
        ]
    );
    let saved = w.saved();
    assert_eq!(saved.apps[&id("blog")].active, Some(v("1.1.0")));
    assert_eq!(saved.apps[&id("blog")].previous, Some(v("1.0.0")));
    assert!(saved.revision > revision);
    assert_eq!(manager.status(), saved);
}

#[test]
fn switch_to_active_is_a_no_op() {
    let w = world(3);
    let manager = w.manager();
    install_and_switch(&manager, "1.0.0");
    let events = w.runtime.events().len();
    manager.switch(&[(id("blog"), v("1.0.0"))]).unwrap();
    assert_eq!(w.runtime.events().len(), events);
}

#[test]
fn switch_refuses_unkept() {
    let w = world(3);
    assert!(matches!(
        w.manager().switch(&[(id("blog"), v("1.0.0"))]),
        Err(ReleaseError::NotKept(..))
    ));
}

#[test]
fn switch_refuses_incompatible() {
    let mut w = World::new();
    w.app("blog", AppKind::Php, 3);
    w.publish("blog", "2.0.0", r#""server":"^9""#);
    let manager = w.manager();
    manager.install(&id("blog"), None).unwrap();
    let err = manager.switch(&[(id("blog"), v("2.0.0"))]).unwrap_err();
    assert!(matches!(err, ReleaseError::Incompatible(_)));
    assert_eq!(
        err.to_string(),
        format!("blog 2.0.0 requires server ^9, active server is {SERVER_VERSION}")
    );
    assert!(w.runtime.events().is_empty());
}

#[test]
fn group_switch_all_or_nothing() {
    let mut w = World::new();
    w.app("blog", AppKind::Php, 3);
    w.app("api", AppKind::Php, 3);
    w.publish("blog", "1.0.0", "");
    w.publish("api", "1.0.0", "");
    w.runtime.fail_prepare("api", "1.0.0", "broken config");
    let manager = w.manager();
    manager.install(&id("blog"), None).unwrap();
    manager.install(&id("api"), None).unwrap();
    let err = manager
        .switch(&[(id("blog"), v("1.0.0")), (id("api"), v("1.0.0"))])
        .unwrap_err();
    assert!(matches!(err, ReleaseError::Prepare { .. }), "{err}");
    assert!(err.to_string().contains("broken config"));
    assert!(
        w.runtime
            .events()
            .iter()
            .all(|e| !e.starts_with("activate")),
        "{:?}",
        w.runtime.events()
    );
    let saved = w.saved();
    assert_eq!(saved.apps[&id("api")].failed[&v("1.0.0")], "broken config");
    assert_eq!(saved.apps[&id("blog")].active, None);
}

#[test]
fn group_switch_satisfies_mutual_requirements() {
    let mut w = World::new();
    w.app("a", AppKind::Php, 3);
    w.app("b", AppKind::Php, 3);
    w.publish("a", "1.0.0", "");
    w.publish("b", "1.0.0", "");
    w.publish("a", "2.0.0", r#""b":"^2""#);
    w.publish("b", "2.0.0", r#""a":"^2""#);
    let manager = w.manager();
    for app in ["a", "b"] {
        manager.install(&id(app), Some(v("1.0.0"))).unwrap();
        manager.switch(&[(id(app), v("1.0.0"))]).unwrap();
        manager.install(&id(app), Some(v("2.0.0"))).unwrap();
    }
    assert!(matches!(
        manager.switch(&[(id("a"), v("2.0.0"))]),
        Err(ReleaseError::Incompatible(_))
    ));
    assert!(matches!(
        manager.switch(&[(id("b"), v("2.0.0"))]),
        Err(ReleaseError::Incompatible(_))
    ));
    manager
        .switch(&[(id("a"), v("2.0.0")), (id("b"), v("2.0.0"))])
        .unwrap();
    assert_eq!(w.runtime.active("a"), Some(v("2.0.0")));
    assert_eq!(w.runtime.active("b"), Some(v("2.0.0")));
}

#[test]
fn group_switch_with_server_needs_keeper() {
    let mut w = world(3);
    w.app("server", AppKind::Server, 3);
    let manager = w.manager();
    manager.install(&id("blog"), Some(v("1.0.0"))).unwrap();
    let err = manager
        .switch(&[(id("blog"), v("1.0.0")), (id("server"), v("4.0.0"))])
        .unwrap_err();
    assert!(matches!(err, ReleaseError::ServerSwitchViaKeeper));
}

#[test]
fn rollback_to_previous() {
    let w = world(3);
    let manager = w.manager();
    install_and_switch(&manager, "1.0.0");
    install_and_switch(&manager, "1.1.0");
    assert_eq!(manager.rollback(&id("blog"), None).unwrap(), v("1.0.0"));
    assert_eq!(w.runtime.active("blog"), Some(v("1.0.0")));
    assert_eq!(w.saved().apps[&id("blog")].previous, Some(v("1.1.0")));
    assert_eq!(
        w.saved().apps[&id("blog")].pinned,
        Some(v("1.0.0")),
        "a rollback pins, so auto-update leaves it"
    );
}

#[test]
fn rollback_stops_auto_reupgrade() {
    let w = world(3);
    let manager = w.manager();
    install_and_switch(&manager, "1.2.0");
    install_and_switch(&manager, "1.3.0");
    manager.rollback(&id("blog"), None).unwrap();
    manager.unpin(&id("blog")).unwrap();
    assert_eq!(
        manager.check(&id("blog")).unwrap(),
        None,
        "rolled-back 1.3.0 must not be offered again"
    );
    assert_eq!(
        w.saved().apps[&id("blog")].failed[&v("1.3.0")],
        "rolled back"
    );
    manager.switch(&[(id("blog"), v("1.3.0"))]).unwrap();
    assert!(
        !w.saved().apps[&id("blog")].failed.contains_key(&v("1.3.0")),
        "an explicit switch clears it"
    );
}

#[test]
fn rollback_named() {
    let w = world(3);
    let manager = w.manager();
    install_and_switch(&manager, "1.0.0");
    install_and_switch(&manager, "1.1.0");
    install_and_switch(&manager, "1.2.0");
    assert_eq!(
        manager.rollback(&id("blog"), Some(v("1.0.0"))).unwrap(),
        v("1.0.0")
    );
    assert_eq!(w.runtime.active("blog"), Some(v("1.0.0")));
}

#[test]
fn rollback_without_previous_with_keep_1() {
    let w = world(1);
    let manager = w.manager();
    install_and_switch(&manager, "1.0.0");
    assert!(matches!(
        manager.rollback(&id("blog"), None),
        Err(ReleaseError::NoPrevious(_))
    ));
    install_and_switch(&manager, "1.1.0");
    assert!(
        !w.release_dir("blog", "1.0.0").exists(),
        "keep = 1 prunes the previous release"
    );
    let err = manager.rollback(&id("blog"), None).unwrap_err();
    assert!(matches!(err, ReleaseError::NoPrevious(_)), "{err}");
}

#[test]
fn prune_keeps_n_and_never_active_pinned_or_previous() {
    let w = world(2);
    let manager = w.manager();
    for version in ["1.0.0", "1.1.0", "1.2.0", "1.3.0"] {
        manager.install(&id("blog"), Some(v(version))).unwrap();
    }
    manager.pin(&id("blog"), v("1.0.0")).unwrap();
    manager.switch(&[(id("blog"), v("1.2.0"))]).unwrap();
    manager.switch(&[(id("blog"), v("1.3.0"))]).unwrap();
    let kept = w.saved().apps[&id("blog")].kept.clone();
    assert_eq!(kept, [v("1.0.0"), v("1.2.0"), v("1.3.0")]);
    assert!(w.release_dir("blog", "1.0.0").exists());
    assert!(!w.release_dir("blog", "1.1.0").exists());
}

#[test]
fn pin_requires_kept() {
    let w = world(3);
    let manager = w.manager();
    assert!(matches!(
        manager.pin(&id("blog"), v("1.0.0")),
        Err(ReleaseError::NotKept(..))
    ));
    manager.install(&id("blog"), Some(v("1.0.0"))).unwrap();
    manager.pin(&id("blog"), v("1.0.0")).unwrap();
    assert_eq!(w.saved().apps[&id("blog")].pinned, Some(v("1.0.0")));
    assert_eq!(manager.check(&id("blog")).unwrap(), None);
    manager.unpin(&id("blog")).unwrap();
    assert_eq!(w.saved().apps[&id("blog")].pinned, None);
    assert_eq!(manager.check(&id("blog")).unwrap(), Some(v("1.3.0")));
}

#[test]
fn operations_serialize() {
    let w = world(3);
    let manager = Arc::new(w.manager());
    manager.install(&id("blog"), Some(v("1.0.0"))).unwrap();
    manager.install(&id("blog"), Some(v("1.1.0"))).unwrap();
    w.runtime.set_prepare_delay(Duration::from_millis(200));
    let threads: Vec<_> = ["1.0.0", "1.1.0"]
        .into_iter()
        .map(|version| {
            let manager = manager.clone();
            std::thread::spawn(move || manager.switch(&[(id("blog"), v(version))]).unwrap())
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let events = w.runtime.events();
    for pair in events.chunks(2).take(2) {
        let version = pair[0].rsplit(' ').next().unwrap();
        assert_eq!(pair[0], format!("prepare blog {version}"), "{events:?}");
        assert_eq!(pair[1], format!("activate blog {version}"), "{events:?}");
    }
    let last = events
        .iter()
        .rev()
        .find(|e| e.starts_with("activate"))
        .unwrap()
        .rsplit(' ')
        .next()
        .unwrap();
    assert_eq!(w.saved().apps[&id("blog")].active, Some(v(last)));
}

#[test]
fn save_failure_does_not_activate() {
    let w = world(3);
    let manager = w.manager();
    install_and_switch(&manager, "1.0.0");
    manager.install(&id("blog"), Some(v("1.1.0"))).unwrap();
    w.state.fail_next_save();
    assert!(matches!(
        manager.switch(&[(id("blog"), v("1.1.0"))]),
        Err(ReleaseError::State(_))
    ));
    assert_eq!(w.runtime.active("blog"), Some(v("1.0.0")));
    assert_eq!(w.saved().apps[&id("blog")].active, Some(v("1.0.0")));
    assert_eq!(manager.status().apps[&id("blog")].active, Some(v("1.0.0")));
}

#[test]
fn off_keeps_a_failed_migration_and_clear_ends_it() {
    let mut w = World::new();
    w.app("blog", AppKind::Php, 3);
    let mut seeded = speedyspec_server_domain::ReleaseState::default();
    seeded.maintenance.insert(
        id("blog"),
        speedyspec_server_domain::MaintenanceRecord {
            operator: false,
            migration_failed: Some("500: boom".into()),
        },
    );
    speedyspec_server_domain::StateStore::save(&*w.state, &seeded).unwrap();
    let manager = w.manager();
    manager.set_maintenance(&[id("blog")], true).unwrap();
    manager.set_maintenance(&[id("blog")], false).unwrap();
    let record = &w.saved().maintenance[&id("blog")];
    assert!(
        !record.operator && record.migration_failed.is_some(),
        "off leaves the failed migration: {record:?}"
    );
    manager.clear_maintenance(&[id("blog")]).unwrap();
    assert!(!w.saved().maintenance.contains_key(&id("blog")));
}

#[test]
fn maintenance_with_no_apps_means_every_app() {
    let mut w = World::new();
    w.app("blog", AppKind::Php, 3);
    w.app("admin", AppKind::Static, 3);
    let apps = w.manager().set_maintenance(&[], true).unwrap();
    assert_eq!(apps, vec![id("admin"), id("blog")]);
    assert!(w.saved().maintenance[&id("admin")].operator);
}
