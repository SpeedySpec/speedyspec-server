mod support;

use speedyspec_server_domain::{AppKind, StateStore};
use support::{World, id, v};

fn world() -> World {
    let mut w = World::new();
    w.app("blog", AppKind::Php, 5);
    for version in ["1.0.0", "1.1.0", "1.2.0"] {
        w.publish("blog", version, "");
    }
    w
}

fn install_all_and_switch(w: &World, active: &str) {
    let manager = w.manager();
    for version in ["1.0.0", "1.1.0", "1.2.0"] {
        manager.install(&id("blog"), Some(v(version))).unwrap();
    }
    manager.switch(&[(id("blog"), v(active))]).unwrap();
}

#[test]
fn boot_activates_the_active_release() {
    let w = world();
    install_all_and_switch(&w, "1.1.0");
    let report = w.manager().boot();
    assert_eq!(w.runtime.active("blog"), Some(v("1.1.0")));
    assert!(
        report.fallbacks.is_empty()
            && report.not_installed.is_empty()
            && report.incompatible.is_empty()
    );
}

#[test]
fn boot_cleans_partials() {
    let w = world();
    let partial = w.dir.path().join("releases/blog/1.2.0.partial");
    std::fs::create_dir_all(partial.join("unpacked")).unwrap();
    let report = w.manager().boot();
    assert_eq!(report.cleaned, [partial.clone()]);
    assert!(!partial.exists());
}

#[test]
fn boot_falls_back_when_active_missing() {
    let w = world();
    install_all_and_switch(&w, "1.2.0");
    std::fs::remove_dir_all(w.release_dir("blog", "1.2.0")).unwrap();
    let report = w.manager().boot();
    assert_eq!(w.runtime.active("blog"), Some(v("1.1.0")));
    assert_eq!(
        report.fallbacks,
        [(id("blog"), Some(v("1.2.0")), Some(v("1.1.0")))]
    );
    let saved = w.saved();
    assert_eq!(saved.apps[&id("blog")].active, Some(v("1.1.0")));
    assert!(!saved.apps[&id("blog")].kept.contains(&v("1.2.0")));
}

#[test]
fn boot_falls_back_when_prepare_fails_and_never_moves_forward() {
    let w = world();
    install_all_and_switch(&w, "1.1.0");
    w.runtime.fail_prepare("blog", "1.1.0", "broken");
    let manager = w.manager();
    let report = manager.boot();
    assert_eq!(
        w.runtime.active("blog"),
        Some(v("1.0.0")),
        "falls back to an older release, not to kept 1.2.0"
    );
    assert_eq!(
        report.fallbacks,
        [(id("blog"), Some(v("1.1.0")), Some(v("1.0.0")))]
    );
    assert_eq!(w.saved().apps[&id("blog")].failed[&v("1.1.0")], "broken");
    assert_eq!(manager.last_boot().unwrap().fallbacks.len(), 1);
}

#[test]
fn boot_reports_not_installed() {
    let w = world();
    let report = w.manager().boot();
    assert_eq!(report.not_installed, [id("blog")]);
    assert!(w.runtime.events().is_empty());
}

#[test]
fn boot_reports_incompatible_but_activates() {
    let mut w = World::new();
    w.app("blog", AppKind::Php, 3);
    w.publish("blog", "2.0.0", r#""server":"^9""#);
    w.manager().install(&id("blog"), None).unwrap();
    let mut state = w.saved();
    state.app_mut(&id("blog")).active = Some(v("2.0.0"));
    w.state.save(&state).unwrap();
    let report = w.manager().boot();
    assert_eq!(w.runtime.active("blog"), Some(v("2.0.0")));
    assert_eq!(report.incompatible.len(), 1);
    assert!(
        report.incompatible[0]
            .to_string()
            .starts_with("blog 2.0.0 requires server ^9")
    );
}

#[test]
fn boot_is_quiet_when_consistent() {
    let w = world();
    install_all_and_switch(&w, "1.1.0");
    let saves = w.state.saves();
    w.manager().boot();
    assert_eq!(w.state.saves(), saves);
}
