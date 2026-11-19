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
