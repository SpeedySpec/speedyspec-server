//! Releases that migrate: under maintenance, between prepare and activate, once.

mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use speedyspec_server_app::{MaintenanceSink, Migrator, ReleaseError, ReleaseManager};
use speedyspec_server_domain::{AppId, AppKind, Prepared};
use support::{World, id, v};

/// Records what it was asked, and which release of the app was active when (as the runtime reports it).
#[derive(Default)]
struct FakeMigrator {
    calls: Mutex<Vec<(String, String, String)>>,
    fail: Mutex<Option<String>>,
    runtime: Mutex<Option<Arc<speedyspec_server_infra_memory::RecordingRuntime>>>,
}

impl Migrator for FakeMigrator {
    fn migrate(&self, prepared: &Prepared, path: &str, _timeout: Duration) -> Result<(), String> {
        let active = self
            .runtime
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|r| r.active(prepared.app.as_str()))
            .map(|v| v.to_string())
            .unwrap_or_default();
        self.calls.lock().unwrap().push((
            prepared.app.to_string(),
            prepared.version.to_string(),
            format!("{path} active={active}"),
        ));
        match self.fail.lock().unwrap().clone() {
            Some(reason) => Err(reason),
            None => Ok(()),
        }
    }
}

#[derive(Default)]
struct Sink(Mutex<Vec<(Vec<String>, bool)>>);

impl MaintenanceSink for Sink {
    fn upgrade(&self, apps: &[AppId], on: bool) {
        self.0
            .lock()
            .unwrap()
            .push((apps.iter().map(|a| a.to_string()).collect(), on));
    }
}

const MIGRATES: &str = r#""php":{"switch":{"maintenance":"self","migrate":"/migrate.php"}}"#;

fn world() -> World {
    let mut w = World::new();
    w.app("blog", AppKind::Php, 3);
    w.publish("blog", "1.0.0", "");
    w.publish_manifest(
        "blog",
        "2.0.0",
        &format!(r#"{{"app":"blog","version":"2.0.0","kind":"php",{MIGRATES}}}"#),
    );
    w
}

fn manager(w: &World, migrator: &Arc<FakeMigrator>, sink: &Arc<Sink>) -> ReleaseManager {
    *migrator.runtime.lock().unwrap() = Some(w.runtime.clone());
    let manager = w
        .manager()
        .with_migrator(migrator.clone())
        .with_maintenance(sink.clone(), vec![id("blog")]);
    for version in ["1.0.0", "2.0.0"] {
        manager.install(&id("blog"), Some(v(version))).unwrap();
    }
    manager.switch(&[(id("blog"), v("1.0.0"))]).unwrap();
    manager
}

#[test]
fn migration_runs_between_prepare_and_activate() {
    let w = world();
    let (migrator, sink) = (Arc::new(FakeMigrator::default()), Arc::new(Sink::default()));
    let manager = manager(&w, &migrator, &sink);
    manager.switch(&[(id("blog"), v("2.0.0"))]).unwrap();
    assert_eq!(
        migrator.calls.lock().unwrap().as_slice(),
        [(
            "blog".into(),
            "2.0.0".into(),
            "/migrate.php active=1.0.0".into()
        )],
        "the old release was still active while it migrated"
    );
    assert_eq!(w.saved().apps[&id("blog")].active, Some(v("2.0.0")));
    assert_eq!(
        sink.0.lock().unwrap().as_slice(),
        [
            (vec!["blog".to_string()], true),
            (vec!["blog".to_string()], false)
        ]
    );
    assert!(
        !w.saved().maintenance.contains_key(&id("blog")),
        "nothing left recorded"
    );
}

#[test]
fn failed_migration_keeps_the_old_release_and_maintenance() {
    let w = world();
    let (migrator, sink) = (Arc::new(FakeMigrator::default()), Arc::new(Sink::default()));
    let manager = manager(&w, &migrator, &sink);
    *migrator.fail.lock().unwrap() = Some("500: boom".into());
    let err = manager.switch(&[(id("blog"), v("2.0.0"))]).unwrap_err();
    assert!(matches!(err, ReleaseError::Migration { .. }), "{err}");
    assert!(
        err.to_string().contains("blog") && err.to_string().contains("500: boom"),
        "{err}"
    );
    let saved = w.saved();
    assert_eq!(saved.apps[&id("blog")].active, Some(v("1.0.0")));
    assert!(saved.apps[&id("blog")].failed.contains_key(&v("2.0.0")));
    assert_eq!(
        saved.maintenance[&id("blog")].migration_failed.as_deref(),
        Some("500: boom")
    );
    assert_eq!(
        sink.0.lock().unwrap().as_slice(),
        [(vec!["blog".to_string()], true)],
        "maintenance stays on"
    );
}

#[test]
fn retry_migration_switches_after_success() {
    let w = world();
    let (migrator, sink) = (Arc::new(FakeMigrator::default()), Arc::new(Sink::default()));
    let manager = manager(&w, &migrator, &sink);
    *migrator.fail.lock().unwrap() = Some("500: boom".into());
    manager.switch(&[(id("blog"), v("2.0.0"))]).unwrap_err();
    *migrator.fail.lock().unwrap() = None;
    manager.retry_migration(&id("blog")).unwrap();
    let saved = w.saved();
    assert_eq!(saved.apps[&id("blog")].active, Some(v("2.0.0")));
    assert!(!saved.maintenance.contains_key(&id("blog")));
    assert_eq!(migrator.calls.lock().unwrap().len(), 2);
}

#[test]
fn runtime_switch_refused_during_migration() {
    let mut w = world();
    w.app("rt", AppKind::Runtime, 3);
    w.publish_binary(
        "rt",
        "1.0.0",
        r#"{"app":"rt","version":"1.0.0","kind":"runtime","contracts":[1]}"#,
        support::TARGET,
    );
    let (migrator, sink) = (Arc::new(FakeMigrator::default()), Arc::new(Sink::default()));
    let manager = manager(&w, &migrator, &sink);
    manager.install(&id("rt"), Some(v("1.0.0"))).unwrap();
    *migrator.fail.lock().unwrap() = Some("500: boom".into());
    manager.switch(&[(id("blog"), v("2.0.0"))]).unwrap_err();
    let err = manager.switch(&[(id("rt"), v("1.0.0"))]).unwrap_err();
    assert!(matches!(err, ReleaseError::MigrationPending(_)), "{err}");
    assert!(
        err.to_string()
            .contains("blog: a migration is pending; retry or clear it first"),
        "{err}"
    );
}
