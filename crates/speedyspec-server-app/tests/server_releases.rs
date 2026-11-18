mod support;

use std::os::unix::fs::PermissionsExt;

use speedyspec_server_app::{ReleaseError, confirm_server, register_running_binary};
use speedyspec_server_domain::{AppKind, PendingGroup, ReleaseState, StateStore};
use speedyspec_server_infra_fs::FsReleaseStore;
use support::{SERVER_VERSION, TARGET, World, id, v};

fn world() -> World {
    let mut w = World::new();
    w.app("server", AppKind::Server, 3);
    w.app("admin", AppKind::Php, 3);
    w
}

fn binary(w: &World, version: &str) -> std::path::PathBuf {
    w.release_dir("server", version).join("speedyspec-server")
}

#[test]
fn installs_server_binary_for_target() {
    let mut w = world();
    w.publish_server("4.0.0", "server", "", "other-target");
    w.publish_server("4.0.0-x", "server", "", TARGET);
    w.app("server", AppKind::Server, 3).prerelease = true;
    let manager = w.manager();
    assert_eq!(
        manager.install(&id("server"), Some(v("4.0.0-x"))).unwrap(),
        v("4.0.0-x")
    );
    let mode = std::fs::metadata(binary(&w, "4.0.0-x"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755);
    assert!(w.saved().apps[&id("server")].kept.contains(&v("4.0.0-x")));
}

#[test]
fn missing_target_artifact_refused() {
    let mut w = world();
    w.publish_server("4.0.0", "server", "", "other-target");
    let err = w
        .manager()
        .install(&id("server"), Some(v("4.0.0")))
        .unwrap_err();
    assert!(
        matches!(err, ReleaseError::NoArtifactForTarget(..)),
        "{err}"
    );
    assert!(err.to_string().contains(TARGET));
}

#[test]
fn server_binary_with_wrong_manifest_refused_and_failed() {
    let mut w = world();
    w.publish_server("4.0.0", "static", "", TARGET);
    let err = w
        .manager()
        .install(&id("server"), Some(v("4.0.0")))
        .unwrap_err();
    assert!(matches!(err, ReleaseError::Inspect(_)), "{err}");
    assert!(!w.release_dir("server", "4.0.0").exists());
    assert!(
        w.saved().apps[&id("server")]
            .failed
            .contains_key(&v("4.0.0"))
    );
}

fn kept_server(
    w: &mut World,
    version: &str,
    requires: &str,
) -> speedyspec_server_app::ReleaseManager {
    w.publish_server(version, "server", requires, TARGET);
    let manager = w.manager();
    manager.install(&id("server"), Some(v(version))).unwrap();
    manager
}

#[test]
fn plain_switch_of_server_goes_through_the_keeper() {
    let mut w = world();
    let manager = kept_server(&mut w, "4.0.0", "");
    assert!(matches!(
        manager.switch(&[(id("server"), v("4.0.0"))]),
        Err(ReleaseError::ServerSwitchViaKeeper)
    ));
}

#[test]
fn begin_server_switch_records_pending_group_and_trial() {
    let mut w = world();
    w.publish("admin", "1.0.0", "");
    w.publish("admin", "2.0.0", "");
    let manager = kept_server(&mut w, "4.0.0", "");
    manager.install(&id("admin"), Some(v("1.0.0"))).unwrap();
    manager.switch(&[(id("admin"), v("1.0.0"))]).unwrap();
    manager.install(&id("admin"), Some(v("2.0.0"))).unwrap();
    let upgrade = manager
        .begin_server_switch(&[(id("server"), v("4.0.0")), (id("admin"), v("2.0.0"))])
        .unwrap();
    assert_eq!(upgrade.version, v("4.0.0"));
    assert_eq!(upgrade.binary, binary(&w, "4.0.0"));
    let saved = w.saved();
    assert_eq!(saved.server.trial, Some(v("4.0.0")));
    assert_eq!(
        saved.pending_group,
        Some(PendingGroup {
            server: (v(SERVER_VERSION), v("4.0.0")),
            apps: vec![(id("admin"), Some(v("1.0.0")), v("2.0.0"))],
        })
    );
    assert_eq!(
        w.runtime.active("admin"),
        Some(v("1.0.0")),
        "nothing activates yet"
    );
}

#[test]
fn begin_server_switch_refused_during_trial() {
    let mut w = world();
    let manager = kept_server(&mut w, "4.0.0", "");
    manager
        .begin_server_switch(&[(id("server"), v("4.0.0"))])
        .unwrap();
    let err = manager
        .begin_server_switch(&[(id("server"), v("4.0.0"))])
        .unwrap_err();
    assert!(matches!(err, ReleaseError::UpgradeInProgress(_)));
    assert!(err.to_string().contains("4.0.0"), "{err}");
}

#[test]
fn begin_server_switch_refuses_incompatible_set() {
    let mut w = world();
    w.publish("admin", "1.0.0", "");
    let manager = kept_server(&mut w, "4.0.0", r#""admin":"^2""#);
    manager.install(&id("admin"), Some(v("1.0.0"))).unwrap();
    manager.switch(&[(id("admin"), v("1.0.0"))]).unwrap();
    let err = manager
        .begin_server_switch(&[(id("server"), v("4.0.0"))])
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "server 4.0.0 requires admin ^2, active admin is 1.0.0"
    );
    assert_eq!(w.saved().pending_group, None);
}

fn with_pending(server_target: &str) -> World {
    let mut w = world();
    w.publish("admin", "1.0.0", "");
    w.publish("admin", "2.0.0", "");
    let manager = w.manager();
    manager.install(&id("admin"), Some(v("1.0.0"))).unwrap();
    manager.switch(&[(id("admin"), v("1.0.0"))]).unwrap();
    manager.install(&id("admin"), Some(v("2.0.0"))).unwrap();
    let mut state = w.saved();
    state.server.trial = Some(v(server_target));
    state.pending_group = Some(PendingGroup {
        server: (v("1.0.0"), v(server_target)),
        apps: vec![(id("admin"), Some(v("1.0.0")), v("2.0.0"))],
    });
    w.state.save(&state).unwrap();
    w
}

#[test]
fn apply_pending_group_switches_apps_for_own_version() {
    let w = with_pending(SERVER_VERSION);
    let manager = w.manager();
    manager.apply_pending_group(&v(SERVER_VERSION)).unwrap();
    assert_eq!(w.runtime.active("admin"), Some(v("2.0.0")));
    let saved = w.saved();
    assert_eq!(saved.apps[&id("admin")].active, Some(v("2.0.0")));
    assert_eq!(saved.apps[&id("server")].active, Some(v(SERVER_VERSION)));
    assert!(saved.pending_group.is_some(), "kept until the trial ends");
}

#[test]
fn apply_pending_group_ignores_other_version() {
    let w = with_pending("9.9.9");
    let before = w.saved();
    w.manager().apply_pending_group(&v(SERVER_VERSION)).unwrap();
    assert_eq!(w.saved(), before);
}

#[test]
fn boot_reverts_group_of_a_failed_trial() {
    let w = with_pending("9.9.9");
    let mut state = w.saved();
    state.app_mut(&id("admin")).active = Some(v("2.0.0"));
    w.state.save(&state).unwrap();
    w.manager().boot();
    assert_eq!(w.runtime.active("admin"), Some(v("1.0.0")));
    let saved = w.saved();
    assert_eq!(saved.apps[&id("admin")].active, Some(v("1.0.0")));
    assert_eq!(saved.pending_group, None);
    assert_eq!(saved.server.trial, None);
    assert!(saved.server.failed.contains_key(&v("9.9.9")));
}

#[test]
fn revert_pending_group_reads_fresh_state() {
    let w = with_pending("9.9.9");
    let manager = w.manager();
    let mut state = w.saved();
    state.app_mut(&id("admin")).active = Some(v("2.0.0"));
    w.state.save(&state).unwrap();
    manager
        .revert_pending_group(&v("9.9.9"), "trial failed")
        .unwrap();
    let saved = w.saved();
    assert_eq!(saved.apps[&id("admin")].active, Some(v("1.0.0")));
    assert_eq!(saved.server.failed[&v("9.9.9")], "trial failed");
    assert_eq!(manager.status(), saved);
}

#[test]
fn confirm_server_clears_trial() {
    let w = with_pending("4.0.0");
    confirm_server(&*w.state, &id("server"), &v("4.0.0")).unwrap();
    let saved = w.saved();
    assert_eq!(saved.server.confirmed, Some(v("4.0.0")));
    assert_eq!((saved.server.trial, saved.pending_group), (None, None));
    assert_eq!(saved.apps[&id("server")].active, Some(v("4.0.0")));
}

#[test]
fn register_running_binary_is_idempotent() {
    let w = World::with_state(ReleaseState::default());
    let exe = w.dir.path().join("exe");
    std::fs::write(&exe, b"binary").unwrap();
    let store = FsReleaseStore::new(w.dir.path());
    let path =
        register_running_binary(&store, &*w.state, &id("server"), &v("1.0.0"), &exe).unwrap();
    assert_eq!(
        path,
        w.release_dir("server", "1.0.0").join("speedyspec-server")
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"binary");
    let saved = w.saved();
    assert_eq!(saved.server.confirmed, Some(v("1.0.0")));
    assert_eq!(saved.apps[&id("server")].kept, [v("1.0.0")]);
    let revision = saved.revision;
    assert_eq!(
        register_running_binary(&store, &*w.state, &id("server"), &v("1.0.0"), &exe).unwrap(),
        path
    );
    assert_eq!(
        w.saved().revision,
        revision,
        "nothing changes the second time"
    );
}
