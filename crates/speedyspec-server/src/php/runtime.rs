//! `PhpRuntime`: what a php release does as it goes live.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use semver::Version;
use speedyspec_server_domain::{AppId, AppRuntime, Prepared, ReleaseManifest};
use speedyspec_server_presentation::{AppHandle, LiveApps};

use super::handler::{self, PhpRequestHandler};
use super::pool::{EmbeddedPhpPool, PoolSettings, prepare_socket_dir};
use super::runtime_kind::{RuntimeBinaries, check_contracts};
use crate::config::PhpConfig;
use speedyspec_runtime_contract::WorkerSpec;

/// Starts each release's worker pool when it is prepared, puts its handler live when it is activated, and drains
/// it when it is retired: a request already running on the old release finishes there.
///
/// Every pool runs the binary of the app's runtime app that is live when the pool is prepared. Switching the runtime
/// prepares new pools for the live releases ([`PhpRuntime::prepare_with`]).
pub struct PhpRuntime {
    live: Arc<LiveApps>,
    binaries: Arc<RuntimeBinaries>,
    run_dir: PathBuf,
    configs: HashMap<AppId, PhpConfig>,
    /// Each php app's runtime app.
    runtimes: HashMap<AppId, AppId>,
    pools: Mutex<HashMap<(AppId, Version), EmbeddedPhpPool>>,
    /// Each app's live release, so a runtime switch can start it again on the new binary.
    releases: Mutex<HashMap<AppId, (ReleaseManifest, PathBuf)>>,
}

pub(crate) struct Ready {
    pub(crate) pool: EmbeddedPhpPool,
    handler: PhpRequestHandler,
    manifest: ReleaseManifest,
    dir: PathBuf,
}

impl PhpRuntime {
    pub fn new(
        live: Arc<LiveApps>,
        binaries: Arc<RuntimeBinaries>,
        run_dir: PathBuf,
        configs: HashMap<AppId, PhpConfig>,
        runtimes: HashMap<AppId, AppId>,
    ) -> PhpRuntime {
        PhpRuntime {
            live,
            binaries,
            run_dir,
            configs,
            runtimes,
            pools: Mutex::new(HashMap::new()),
            releases: Mutex::new(HashMap::new()),
        }
    }

    /// The php apps on runtime `runtime` that have a live release, with that release.
    pub(crate) fn live_on(&self, runtime: &AppId) -> Vec<(AppId, ReleaseManifest, PathBuf)> {
        let releases = self.releases.lock().unwrap();
        let mut apps: Vec<_> = releases
            .iter()
            .filter(|(app, _)| self.runtimes.get(*app) == Some(runtime))
            .map(|(app, (manifest, dir))| (app.clone(), manifest.clone(), dir.clone()))
            .collect();
        apps.sort_by(|a, b| a.0.cmp(&b.0));
        apps
    }

    /// The binary `app`'s pools run: its runtime app's live binary, which must speak this server's contract.
    fn binary_for(&self, app: &AppId) -> Result<PathBuf, String> {
        let runtime = self
            .runtimes
            .get(app)
            .ok_or_else(|| format!("{app}: no runtime app is configured for it"))?;
        let (_, binary, contracts) = self
            .binaries
            .active(runtime)
            .ok_or_else(|| format!("{app}: runtime {runtime} has no active release"))?;
        check_contracts(&contracts).map_err(|err| format!("{app}: runtime {runtime} {err}"))?;
        Ok(binary)
    }

    /// Prepares `app`'s release on `binary` (a runtime's new binary, during a runtime switch).
    pub(crate) fn prepare_with(
        &self,
        app: &AppId,
        manifest: &ReleaseManifest,
        dir: &Path,
        binary: &Path,
    ) -> Result<Prepared, String> {
        let ready = self.ready(app, manifest, dir, binary)?;
        Ok(Prepared {
            app: app.clone(),
            version: manifest.version.clone(),
            payload: Box::new(ready),
        })
    }

    fn config(&self, app: &AppId) -> PhpConfig {
        self.configs.get(app).cloned().unwrap_or(PhpConfig {
            workers: crate::config::DEFAULT_PHP_WORKERS,
            max_requests: crate::config::DEFAULT_PHP_MAX_REQUESTS,
            request_timeout: crate::config::DEFAULT_PHP_REQUEST_TIMEOUT,
            memory_limit: crate::config::DEFAULT_PHP_MEMORY_LIMIT,
            idle_timeout: crate::config::DEFAULT_PHP_IDLE_TIMEOUT,
            site_dir: None,
            bindings: Vec::new(),
        })
    }

    /// Starts `app`'s pool for the release in `dir` and builds its handler (immutable mode calls this directly).
    pub fn start(
        &self,
        app: &AppId,
        manifest: &ReleaseManifest,
        dir: &Path,
    ) -> Result<AppHandle, String> {
        let binary = self.binary_for(app)?;
        let ready = self.ready(app, manifest, dir, &binary)?;
        let handle = AppHandle::Php(handler::arc(ready.handler));
        if let Some(old) = self
            .pools
            .lock()
            .unwrap()
            .insert((app.clone(), manifest.version.clone()), ready.pool)
        {
            old.drain();
        }
        Ok(handle)
    }

    fn ready(
        &self,
        app: &AppId,
        manifest: &ReleaseManifest,
        dir: &Path,
        binary: &Path,
    ) -> Result<Ready, String> {
        let section = manifest
            .php_section()
            .ok_or("the release is not a php release")?;
        let config = self.config(app);
        let release_dir =
            std::fs::canonicalize(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
        let document_root = release_dir.join(&section.document_root);
        if !document_root.join(&section.entry).is_file() {
            return Err(format!(
                "the php entry {}/{} is not in the release",
                section.document_root, section.entry
            ));
        }
        let spec = WorkerSpec {
            contract: speedyspec_runtime_contract::CONTRACT_MAJOR,
            env: Vec::new(),
            app: app.to_string(),
            socket: PathBuf::new(),
            release_dir: release_dir.clone(),
            site_dir: config.site_dir.clone(),
            ini: section.ini.clone(),
            bindings: config.bindings.clone(),
        };
        let version = manifest.version.to_string();
        let sockets = prepare_socket_dir(&self.run_dir, app.as_str(), &version)?;
        let pool = EmbeddedPhpPool::start(
            binary.to_path_buf(),
            spec,
            sockets,
            PoolSettings::from_config(&config),
            version,
        )?;
        let handler = PhpRequestHandler::new(
            pool.clone(),
            &document_root,
            &section.entry,
            config.site_dir.clone(),
        )
        .map_err(|err| format!("{}: {err}", document_root.display()))?;
        Ok(Ready {
            pool,
            handler,
            manifest: manifest.clone(),
            dir: dir.to_path_buf(),
        })
    }
}

impl AppRuntime for PhpRuntime {
    fn prepare(
        &self,
        app: &AppId,
        manifest: &ReleaseManifest,
        dir: &Path,
    ) -> Result<Prepared, String> {
        let binary = self.binary_for(app)?;
        let ready = self.ready(app, manifest, dir, &binary)?;
        Ok(Prepared {
            app: app.clone(),
            version: manifest.version.clone(),
            payload: Box::new(ready),
        })
    }

    fn activate(&self, prepared: Prepared) {
        let ready = *prepared
            .payload
            .downcast::<Ready>()
            .expect("prepared by PhpRuntime");
        let replaced = self
            .pools
            .lock()
            .unwrap()
            .insert((prepared.app.clone(), prepared.version.clone()), ready.pool);
        // A reactivation of the live version replaces its pool: the old one drains.
        if let Some(old) = replaced {
            old.drain();
        }
        self.releases
            .lock()
            .unwrap()
            .insert(prepared.app.clone(), (ready.manifest, ready.dir));
        self.live
            .swap(&prepared.app, AppHandle::Php(handler::arc(ready.handler)));
    }

    fn retire(&self, app: &AppId, version: &Version) {
        if let Some(pool) = self
            .pools
            .lock()
            .unwrap()
            .remove(&(app.clone(), version.clone()))
        {
            pool.drain();
        }
    }
}
