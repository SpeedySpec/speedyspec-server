mod support;

use speedyspec_server_app::{ReleaseError, Signing};
use speedyspec_server_domain::{AppKind, ReleaseState};
use support::{World, id, v};

fn world() -> World {
    let mut w = World::new();
    w.app("blog", AppKind::Php, 3);
    w.publish("blog", "1.0.0", "");
    w.publish("blog", "1.1.0", "");
    w
}

#[test]
fn installs_newest_and_keeps_it() {
    let w = world();
    let manager = w.manager();
    assert_eq!(manager.install(&id("blog"), None).unwrap(), v("1.1.0"));
    let state = w.saved();
    assert_eq!(state.apps[&id("blog")].kept, [v("1.1.0")]);
    assert_eq!(state.apps[&id("blog")].active, None);
    assert!(w.release_dir("blog", "1.1.0").join("index.php").exists());
    assert!(state.revision > 0);
}

#[test]
fn install_named_version() {
    let w = world();
    assert_eq!(
        w.manager().install(&id("blog"), Some(v("1.0.0"))).unwrap(),
        v("1.0.0")
    );
    assert!(w.release_dir("blog", "1.0.0").exists());
}

#[test]
fn install_unknown_version_is_not_in_index() {
    let w = world();
    assert!(matches!(
        w.manager().install(&id("blog"), Some(v("9.9.9"))),
        Err(ReleaseError::NotInIndex(..))
    ));
}

#[test]
fn nothing_newer() {
    let mut state = ReleaseState::default();
    state.app_mut(&id("blog")).active = Some(v("1.1.0"));
    let mut w = World::with_state(state);
    w.app("blog", AppKind::Php, 3);
    w.publish("blog", "1.1.0", "");
    let manager = w.manager();
    assert_eq!(manager.check(&id("blog")).unwrap(), None);
    assert!(matches!(
        manager.install(&id("blog"), None),
        Err(ReleaseError::NothingNewer(_))
    ));
}

#[test]
fn check_skips_pinned_and_failed() {
    let mut state = ReleaseState::default();
    state
        .app_mut(&id("blog"))
        .failed
        .insert(v("1.1.0"), "broken".to_string());
    let mut w = World::with_state(state);
    w.app("blog", AppKind::Php, 3);
    w.publish("blog", "1.0.0", "");
    w.publish("blog", "1.1.0", "");
    assert_eq!(w.manager().check(&id("blog")).unwrap(), Some(v("1.0.0")));

    let mut pinned = ReleaseState::default();
    pinned.app_mut(&id("blog")).pinned = Some(v("1.0.0"));
    let mut w = World::with_state(pinned);
    w.app("blog", AppKind::Php, 3);
    w.publish("blog", "1.1.0", "");
    assert_eq!(w.manager().check(&id("blog")).unwrap(), None);
}

#[test]
fn checksum_mismatch_keeps_nothing() {
    let mut w = world();
    w.corrupt_checksum("blog", "1.1.0");
    let before = w.saved();
    match w.manager().install(&id("blog"), None) {
        Err(ReleaseError::Checksum {
            expected, actual, ..
        }) => {
            assert_eq!(expected, "0".repeat(64));
            assert_ne!(actual, expected);
        }
        other => panic!("expected Checksum, got {other:?}"),
    }
    assert!(!w.dir.path().join("releases/blog/1.1.0.partial").exists());
    assert!(!w.release_dir("blog", "1.1.0").exists());
    let mut after = w.saved();
    assert!(after.apps[&id("blog")].failed[&v("1.1.0")].contains("checksum"));
    after.apps.get_mut(&id("blog")).unwrap().failed.clear();
    after.revision = before.revision;
    assert_eq!(
        after.apps[&id("blog")].kept,
        before
            .apps
            .get(&id("blog"))
            .map(|a| a.kept.clone())
            .unwrap_or_default()
    );
}

#[test]
fn failed_install_is_not_offered_again() {
    let mut w = world();
    w.corrupt_checksum("blog", "1.1.0");
    let manager = w.manager();
    assert!(manager.install(&id("blog"), None).is_err());
    assert_eq!(
        manager.check(&id("blog")).unwrap(),
        Some(v("1.0.0")),
        "the failed 1.1.0 is skipped"
    );
    let downloads = w.source.downloads().len();
    manager.install(&id("blog"), None).unwrap();
    assert!(
        !w.source.downloads()[downloads..]
            .iter()
            .any(|u| u.contains("1.1.0")),
        "1.1.0 is not downloaded again"
    );
}

#[test]
fn network_failures_are_not_recorded() {
    let w = world();
    w.source
        .fail_next_download(&w.artifact_url("blog", "1.1.0"), 10);
    let manager = w.manager();
    assert!(manager.install(&id("blog"), None).is_err());
    assert_eq!(
        manager.check(&id("blog")).unwrap(),
        Some(v("1.1.0")),
        "a dropped connection is retried"
    );
}

#[test]
fn interrupted_download_resumes() {
    let w = world();
    let url = w.artifact_url("blog", "1.1.0");
    w.source.fail_next_download(&url, 100);
    let manager = w.manager();
    assert!(manager.install(&id("blog"), None).is_err());
    assert_eq!(manager.install(&id("blog"), None).unwrap(), v("1.1.0"));
    assert_eq!(
        w.source.downloads().iter().filter(|u| **u == url).count(),
        2
    );
}

#[test]
fn bad_signature_refused() {
    let w = world();
    w.bad_signature("blog", "1.1.0");
    assert!(matches!(
        w.manager().install(&id("blog"), None),
        Err(ReleaseError::Signature(_))
    ));
    assert!(!w.release_dir("blog", "1.1.0").exists());
    assert!(!w.dir.path().join("releases/blog/1.1.0.partial").exists());
}

#[test]
fn unsigned_refused_without_allow_unsigned() {
    let mut w = world();
    w.app("blog", AppKind::Php, 3).signing = Signing::NoKeys;
    let err = w.manager().install(&id("blog"), None).unwrap_err();
    assert!(matches!(err, ReleaseError::Unsigned(_)));
    let message = err.to_string();
    assert!(
        message.contains("keys") && message.contains("allow_unsigned"),
        "{message}"
    );
    assert!(w.source.downloads().is_empty());
}

#[test]
fn unsigned_allowed_skips_signature() {
    let mut w = world();
    w.app("blog", AppKind::Php, 3).signing = Signing::AllowUnsigned;
    w.bad_signature("blog", "1.1.0");
    w.manager().install(&id("blog"), None).unwrap();
    assert!(
        w.source
            .downloads()
            .iter()
            .all(|u| !u.ends_with(".minisig"))
    );
}

#[test]
fn index_for_other_app_refused() {
    let w = world();
    w.source
        .add_index(&support::url("blog"), r#"{"app":"admin","releases":[]}"#);
    let err = w
        .manager()
        .install(&id("blog"), None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("admin"), "{err}");
}

#[test]
fn kind_mismatch_refused() {
    let mut w = World::new();
    w.app("blog", AppKind::Static, 3);
    w.publish("blog", "1.0.0", "");
    let err = w
        .manager()
        .install(&id("blog"), None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("php") && err.contains("static"), "{err}");
    assert!(!w.release_dir("blog", "1.0.0").exists());
}

#[test]
fn prerelease_never_candidate_unless_enabled() {
    let mut w = World::new();
    w.app("blog", AppKind::Php, 3);
    w.publish("blog", "1.0.0", "");
    w.publish("blog", "1.1.0-rc.1", "");
    assert_eq!(w.manager().check(&id("blog")).unwrap(), Some(v("1.0.0")));
    w.app("blog", AppKind::Php, 3).prerelease = true;
    assert_eq!(
        w.manager().check(&id("blog")).unwrap(),
        Some(v("1.1.0-rc.1"))
    );
}

#[test]
fn keep_zero_rejected() {
    let mut w = World::new();
    w.app("blog", AppKind::Php, 0);
    let err = speedyspec_server_app::ReleaseManager::new(
        w.policies.clone(),
        support::server_manifest(),
        w.source.clone(),
        std::sync::Arc::new(speedyspec_server_infra_memory::MemoryVerifier),
        std::sync::Arc::new(speedyspec_server_infra_fs::FsReleaseStore::new(
            w.dir.path(),
        )),
        w.state.clone(),
        Default::default(),
        std::sync::Arc::new(speedyspec_server_infra_memory::FakeInspector),
        support::TARGET.to_string(),
    )
    .err()
    .unwrap();
    assert!(matches!(err, ReleaseError::KeepTooSmall(_)));
}
