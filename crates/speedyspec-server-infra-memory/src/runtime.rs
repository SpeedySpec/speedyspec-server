//! An app runtime that records what it was asked to do.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use semver::Version;
use speedyspec_server_domain::{AppId, AppRuntime, Prepared, ReleaseManifest};

/// Records every call as `"prepare admin 1.0.0"`, `"activate admin 1.0.0"` or `"retire admin 1.0.0"`, and can be told
/// to fail a prepare or to be slow.
#[derive(Default)]
pub struct RecordingRuntime {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    events: Vec<String>,
    failing: HashMap<(String, String), String>,
    active: HashMap<String, Version>,
    delay: Duration,
}

impl RecordingRuntime {
    pub fn new() -> RecordingRuntime {
        RecordingRuntime::default()
    }

    pub fn fail_prepare(&self, app: &str, version: &str, reason: &str) {
        let key = (app.to_string(), version.to_string());
        self.inner
            .lock()
            .unwrap()
            .failing
            .insert(key, reason.to_string());
    }

    /// Every prepare sleeps this long first.
    pub fn set_prepare_delay(&self, delay: Duration) {
        self.inner.lock().unwrap().delay = delay;
    }

    pub fn events(&self) -> Vec<String> {
        self.inner.lock().unwrap().events.clone()
    }

    pub fn active(&self, app: &str) -> Option<Version> {
        self.inner.lock().unwrap().active.get(app).cloned()
    }
}

impl AppRuntime for RecordingRuntime {
    fn prepare(
        &self,
        app: &AppId,
        manifest: &ReleaseManifest,
        _dir: &Path,
    ) -> Result<Prepared, String> {
        let delay = self.inner.lock().unwrap().delay;
        std::thread::sleep(delay);
        let mut inner = self.inner.lock().unwrap();
        let key = (app.to_string(), manifest.version.to_string());
        if let Some(reason) = inner.failing.get(&key).cloned() {
            inner
                .events
                .push(format!("prepare {app} {} failed", manifest.version));
            return Err(reason);
        }
        inner
            .events
            .push(format!("prepare {app} {}", manifest.version));

        Ok(Prepared {
            app: app.clone(),
            version: manifest.version.clone(),
            payload: Box::new(()),
        })
    }

    fn activate(&self, prepared: Prepared) {
        let mut inner = self.inner.lock().unwrap();
        inner
            .events
            .push(format!("activate {} {}", prepared.app, prepared.version));
        inner
            .active
            .insert(prepared.app.to_string(), prepared.version);
    }

    fn retire(&self, app: &AppId, version: &Version) {
        self.inner
            .lock()
            .unwrap()
            .events
            .push(format!("retire {app} {version}"));
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use speedyspec_server_domain::{AppKind, ReleaseManifest};

    use super::*;

    fn manifest(version: &str) -> ReleaseManifest {
        ReleaseManifest {
            app: AppId::parse("admin").unwrap(),
            version: Version::parse(version).unwrap(),
            kind: AppKind::Php,
            requires: Default::default(),
            static_: None,
            php: None,
            protocol: None,
        }
    }

    #[test]
    fn recording_runtime_records_order() {
        let runtime = RecordingRuntime::new();
        let admin = AppId::parse("admin").unwrap();
        runtime.fail_prepare("admin", "2.0.0", "broken");
        assert_eq!(
            runtime
                .prepare(&admin, &manifest("2.0.0"), Path::new("/x"))
                .err()
                .unwrap(),
            "broken"
        );
        let prepared = runtime
            .prepare(&admin, &manifest("1.0.0"), Path::new("/x"))
            .unwrap();
        runtime.activate(prepared);
        runtime.retire(&admin, &Version::new(0, 9, 0));
        assert_eq!(
            runtime.events(),
            [
                "prepare admin 2.0.0 failed",
                "prepare admin 1.0.0",
                "activate admin 1.0.0",
                "retire admin 0.9.0"
            ]
        );
        assert_eq!(runtime.active("admin"), Some(Version::new(1, 0, 0)));
    }
}
