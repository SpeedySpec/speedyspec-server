//! Runtime releases: binaries per target, like the server's, kept with their manifest.

mod support;

use std::os::unix::fs::PermissionsExt;

use speedyspec_server_app::ReleaseError;
use speedyspec_server_domain::{AppKind, ReleaseStore};
use speedyspec_server_infra_fs::FsReleaseStore;
use support::{TARGET, World, id, v};

fn world() -> World {
    let mut w = World::new();
    w.app("server", AppKind::Server, 3);
    w.app("rt", AppKind::Runtime, 3);
    w
}

fn runtime_manifest(version: &str) -> String {
    format!(r#"{{"app":"rt","version":"{version}","kind":"runtime","contracts":[1]}}"#)
}

#[test]
fn runtime_release_installs_the_target_artifact() {
    let mut w = world();
    w.publish_binary("rt", "1.0.0", &runtime_manifest("1.0.0"), "other-target");
    w.publish_binary("rt", "1.1.0", &runtime_manifest("1.1.0"), TARGET);
    let manager = w.manager();
    assert_eq!(
        manager.install(&id("rt"), Some(v("1.1.0"))).unwrap(),
        v("1.1.0")
    );
    let binary = w.release_dir("rt", "1.1.0").join("speedyspec-php-runtime");
    assert_eq!(
        std::fs::metadata(&binary).unwrap().permissions().mode() & 0o777,
        0o755
    );
    let kept = FsReleaseStore::new(w.dir.path())
        .manifest(&id("rt"), &v("1.1.0"))
        .unwrap();
    assert_eq!(
        (kept.kind, kept.contracts.clone()),
        (AppKind::Runtime, vec![1])
    );
    assert!(w.saved().apps[&id("rt")].kept.contains(&v("1.1.0")));
    let err = manager.install(&id("rt"), Some(v("1.0.0"))).unwrap_err();
    assert!(
        matches!(err, ReleaseError::NoArtifactForTarget(..)),
        "{err}"
    );
}

#[test]
fn runtime_artifact_whose_manifest_differs_is_refused() {
    let mut w = world();
    w.publish_binary(
        "rt",
        "1.0.0",
        r#"{"app":"server","version":"1.0.0","kind":"server"}"#,
        TARGET,
    );
    let err = w
        .manager()
        .install(&id("rt"), Some(v("1.0.0")))
        .unwrap_err();
    assert!(matches!(err, ReleaseError::Inspect(_)), "{err}");
    assert!(!w.release_dir("rt", "1.0.0").exists());
    assert!(w.saved().apps[&id("rt")].failed.contains_key(&v("1.0.0")));
}

#[test]
fn runtime_and_its_php_app_cannot_switch_together() {
    let mut w = world();
    w.app("blog", AppKind::Php, 3);
    w.publish_binary("rt", "1.0.0", &runtime_manifest("1.0.0"), TARGET);
    w.publish("blog", "1.0.0", "");
    let manager = w
        .manager()
        .with_php_runtimes([(id("blog"), id("rt"))].into());
    manager.install(&id("rt"), Some(v("1.0.0"))).unwrap();
    manager.install(&id("blog"), Some(v("1.0.0"))).unwrap();
    let err = manager
        .switch(&[(id("rt"), v("1.0.0")), (id("blog"), v("1.0.0"))])
        .unwrap_err();
    assert!(
        matches!(err, ReleaseError::RuntimeWithItsApps { .. }),
        "{err}"
    );
    assert!(
        err.to_string().contains("rt") && err.to_string().contains("blog"),
        "{err}"
    );
    manager.switch(&[(id("rt"), v("1.0.0"))]).unwrap();
    manager.switch(&[(id("blog"), v("1.0.0"))]).unwrap();
}

/// rt 1.0.0 then 1.1.0 kept, blog on rt, the runtime switched to 1.0.0 then to 1.1.0 (on trial for `trial`).
fn on_trial(trial: std::time::Duration) -> (World, speedyspec_server_app::ReleaseManager) {
    let mut w = world();
    w.app("blog", AppKind::Php, 3);
    w.publish_binary("rt", "1.0.0", &runtime_manifest("1.0.0"), TARGET);
    w.publish_binary("rt", "1.1.0", &runtime_manifest("1.1.0"), TARGET);
    let manager = w
        .manager()
        .with_php_runtimes([(id("blog"), id("rt"))].into())
        .with_runtime_trial_period(trial);
    for version in ["1.0.0", "1.1.0"] {
        manager.install(&id("rt"), Some(v(version))).unwrap();
    }
    manager.switch(&[(id("rt"), v("1.0.0"))]).unwrap();
    assert!(
        w.saved().runtime_trials.is_empty(),
        "a first runtime has nothing to fall back to"
    );
    manager.switch(&[(id("rt"), v("1.1.0"))]).unwrap();
    let trial_record = &w.saved().runtime_trials[&id("rt")];
    assert_eq!(
        (trial_record.version.clone(), trial_record.previous.clone()),
        (v("1.1.0"), v("1.0.0"))
    );
    (w, manager)
}

#[test]
fn unhealthy_during_trial_rolls_back_and_marks_failed() {
    let (w, manager) = on_trial(std::time::Duration::from_secs(60));
    assert!(manager.runtime_unhealthy(&id("blog")).unwrap());
    let saved = w.saved();
    assert_eq!(saved.apps[&id("rt")].active, Some(v("1.0.0")));
    assert!(saved.apps[&id("rt")].failed[&v("1.1.0")].contains("unhealthy during trial: blog"));
    assert!(saved.runtime_trials.is_empty());
}

#[test]
fn unhealthy_after_trial_does_not_roll_back() {
    let (w, manager) = on_trial(std::time::Duration::from_millis(100));
    std::thread::sleep(std::time::Duration::from_millis(250));
    assert!(!manager.runtime_unhealthy(&id("blog")).unwrap());
    assert_eq!(w.saved().apps[&id("rt")].active, Some(v("1.1.0")));
}

#[test]
fn trial_is_confirmed_after_the_period() {
    let (w, manager) = on_trial(std::time::Duration::from_millis(100));
    manager.confirm_runtime_trials().unwrap();
    assert!(!w.saved().runtime_trials.is_empty(), "still on trial");
    std::thread::sleep(std::time::Duration::from_millis(250));
    manager.confirm_runtime_trials().unwrap();
    assert!(w.saved().runtime_trials.is_empty(), "confirmed");
}

#[test]
fn a_failed_runtime_is_not_auto_updated_again() {
    let (w, manager) = on_trial(std::time::Duration::from_secs(60));
    manager.runtime_unhealthy(&id("blog")).unwrap();
    assert!(w.saved().apps[&id("rt")].failed.contains_key(&v("1.1.0")));
    assert_eq!(
        manager.check(&id("rt")).unwrap(),
        None,
        "the failed version is not offered again"
    );
}

#[test]
fn independent_replica_rolls_back_only_itself() {
    let (w1, one) = on_trial(std::time::Duration::from_secs(60));
    let (w2, _two) = on_trial(std::time::Duration::from_secs(60));
    one.runtime_unhealthy(&id("blog")).unwrap();
    assert_eq!(w1.saved().apps[&id("rt")].active, Some(v("1.0.0")));
    assert_eq!(
        w2.saved().apps[&id("rt")].active,
        Some(v("1.1.0")),
        "the other replica has its own state"
    );
}

#[test]
fn trial_rollback_is_not_blocked_by_a_failed_migration() {
    let (w, manager) = on_trial(std::time::Duration::from_secs(60));
    let mut state = w.saved();
    state.revision += 1;
    state.maintenance.insert(
        id("blog"),
        speedyspec_server_domain::MaintenanceRecord {
            migration_failed: Some("500".into()),
            ..Default::default()
        },
    );
    speedyspec_server_domain::StateStore::save(&*w.state, &state).unwrap();
    manager.refresh().unwrap();
    assert!(
        manager.runtime_unhealthy(&id("blog")).unwrap(),
        "a trial rollback is not an operator's switch"
    );
    assert_eq!(w.saved().apps[&id("rt")].active, Some(v("1.0.0")));
}

#[test]
fn an_app_already_unhealthy_rolls_back_the_trial() {
    let (w, manager) = on_trial(std::time::Duration::from_secs(60));
    // The app never turns unhealthy during the trial: it already was. The round's check still rolls back.
    manager.check_runtime_trials(&[id("blog")]).unwrap();
    assert_eq!(w.saved().apps[&id("rt")].active, Some(v("1.0.0")));
}

#[test]
fn a_trial_is_never_confirmed_while_an_app_on_it_is_unhealthy() {
    let (w, manager) = on_trial(std::time::Duration::from_millis(100));
    std::thread::sleep(std::time::Duration::from_millis(250));
    manager.check_runtime_trials(&[id("blog")]).unwrap();
    assert_eq!(
        w.saved().apps[&id("rt")].active,
        Some(v("1.0.0")),
        "rolled back, not confirmed"
    );
    let (w, manager) = on_trial(std::time::Duration::from_millis(100));
    std::thread::sleep(std::time::Duration::from_millis(250));
    manager.check_runtime_trials(&[]).unwrap();
    assert!(
        w.saved().runtime_trials.is_empty(),
        "confirmed when healthy"
    );
    assert_eq!(w.saved().apps[&id("rt")].active, Some(v("1.1.0")));
}
