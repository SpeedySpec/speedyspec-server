//! The `runtime` kind's releases: a runtime binary goes live by moving every PHP app on it to new pools.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use semver::Version;
use speedyspec_runtime_contract::CONTRACT_MAJOR;
use speedyspec_server_domain::{AppId, AppKind, AppRuntime, Prepared, ReleaseManifest};
#[cfg(test)]
use speedyspec_server_presentation::LiveApps;

use super::runtime::{PhpRuntime, Ready};

/// Each runtime app's live binary, with its version and the contract majors it speaks.
#[derive(Default)]
pub struct RuntimeBinaries {
    active: Mutex<HashMap<AppId, (Version, PathBuf, Vec<u32>)>>,
}

impl RuntimeBinaries {
    pub fn active(&self, runtime: &AppId) -> Option<(Version, PathBuf, Vec<u32>)> {
        self.active.lock().unwrap().get(runtime).cloned()
    }

    pub fn set(&self, runtime: &AppId, version: Version, binary: PathBuf, contracts: Vec<u32>) {
        self.active
            .lock()
            .unwrap()
            .insert(runtime.clone(), (version, binary, contracts));
    }
}

/// Refuses a runtime that does not speak this server's worker contract.
pub(crate) fn check_contracts(contracts: &[u32]) -> Result<(), String> {
    if contracts.contains(&CONTRACT_MAJOR) {
        return Ok(());
    }
    let spoken: Vec<String> = contracts
        .iter()
        .map(|major| format!("contract {major}"))
        .collect();
    Err(format!(
        "speaks {}, this server speaks contract {CONTRACT_MAJOR}",
        if spoken.is_empty() {
            "no contract".to_string()
        } else {
            spoken.join(", ")
        }
    ))
}

/// A runtime release made ready: its binary, and every php app on it prepared on that binary.
struct RuntimeReady {
    binary: PathBuf,
    contracts: Vec<u32>,
    apps: Vec<Prepared>,
}

/// Goes live by starting every php app on the runtime again from the new binary, then swapping them all at once; the
/// old pools drain as each app's new pool replaces them.
pub struct RuntimeKindRuntime {
    php: Arc<PhpRuntime>,
    pub(crate) binaries: Arc<RuntimeBinaries>,
}

impl RuntimeKindRuntime {
    pub fn new(php: Arc<PhpRuntime>, binaries: Arc<RuntimeBinaries>) -> RuntimeKindRuntime {
        RuntimeKindRuntime { php, binaries }
    }
}

impl AppRuntime for RuntimeKindRuntime {
    fn prepare(
        &self,
        app: &AppId,
        manifest: &ReleaseManifest,
        dir: &Path,
    ) -> Result<Prepared, String> {
        check_contracts(&manifest.contracts)
            .map_err(|err| format!("runtime {app} {} {err}", manifest.version))?;
        let binary = dir.join(AppKind::Runtime.binary_name().expect("a binary kind"));
        let mut apps = Vec::new();
        for (php_app, release, release_dir) in self.php.live_on(app) {
            match self
                .php
                .prepare_with(&php_app, &release, &release_dir, &binary)
            {
                Ok(prepared) => apps.push(prepared),
                Err(err) => {
                    for started in apps {
                        if let Ok(ready) = started.payload.downcast::<Ready>() {
                            ready.pool.drain();
                        }
                    }
                    return Err(format!(
                        "{php_app} on runtime {app} {}: {err}",
                        manifest.version
                    ));
                }
            }
        }
        Ok(Prepared {
            app: app.clone(),
            version: manifest.version.clone(),
            payload: Box::new(RuntimeReady {
                binary,
                contracts: manifest.contracts.clone(),
                apps,
            }),
        })
    }

    fn activate(&self, prepared: Prepared) {
        let ready = *prepared
            .payload
            .downcast::<RuntimeReady>()
            .expect("prepared by RuntimeKindRuntime");
        self.binaries.set(
            &prepared.app,
            prepared.version,
            ready.binary,
            ready.contracts,
        );
        for app in ready.apps {
            self.php.activate(app);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(contracts: Vec<u32>) -> ReleaseManifest {
        ReleaseManifest {
            app: AppId::parse("rt").unwrap(),
            version: semver::Version::new(1, 1, 0),
            kind: speedyspec_server_domain::AppKind::Runtime,
            requires: Default::default(),
            static_: None,
            php: None,
            protocol: None,
            contracts,
        }
    }

    fn kind() -> RuntimeKindRuntime {
        let binaries = Arc::new(RuntimeBinaries::default());
        let php = Arc::new(PhpRuntime::new(
            Arc::new(LiveApps::new(Default::default())),
            binaries.clone(),
            std::env::temp_dir(),
            Default::default(),
            Default::default(),
        ));
        RuntimeKindRuntime::new(php, binaries)
    }

    #[test]
    fn contract_mismatch_is_refused_naming_both() {
        let dir = tempfile::tempdir().unwrap();
        let err = kind()
            .prepare(&AppId::parse("rt").unwrap(), &runtime(vec![2]), dir.path())
            .err()
            .expect("refused");
        assert!(
            err.contains("contract 2") && err.contains("contract 1"),
            "{err}"
        );
    }

    #[test]
    fn a_runtime_with_no_apps_on_it_goes_live() {
        let dir = tempfile::tempdir().unwrap();
        let kind = kind();
        let rt = AppId::parse("rt").unwrap();
        let prepared = kind.prepare(&rt, &runtime(vec![1]), dir.path()).unwrap();
        kind.activate(prepared);
        let (version, binary, _) = kind.binaries.active(&rt).unwrap();
        assert_eq!(version, semver::Version::new(1, 1, 0));
        assert_eq!(binary, dir.path().join("speedyspec-php-runtime"));
    }
}
