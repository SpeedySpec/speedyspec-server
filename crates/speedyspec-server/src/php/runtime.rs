//! `PhpRuntime`: what a php release does as it goes live.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use semver::Version;
use speedyspec_server_domain::{AppId, AppRuntime, Prepared, ReleaseManifest};
use speedyspec_server_presentation::{AppHandle, LiveApps};

use super::handler::{self, PhpRequestHandler};
use super::pool::{EmbeddedPhpPool, PoolSettings, prepare_socket_dir};
use super::worker::WorkerSpec;
use crate::config::PhpConfig;

/// Starts each release's worker pool when it is prepared, puts its handler live when it is activated, and drains
/// it when it is retired: a request already running on the old release finishes there.
pub struct PhpRuntime {
    live: Arc<LiveApps>,
    exe: PathBuf,
    run_dir: PathBuf,
    configs: HashMap<AppId, PhpConfig>,
    pools: Mutex<HashMap<(AppId, Version), EmbeddedPhpPool>>,
}

struct Ready {
    pool: EmbeddedPhpPool,
    handler: PhpRequestHandler,
}

impl PhpRuntime {
    pub fn new(
        live: Arc<LiveApps>,
        exe: PathBuf,
        run_dir: PathBuf,
        configs: HashMap<AppId, PhpConfig>,
    ) -> PhpRuntime {
        PhpRuntime {
            live,
            exe,
            run_dir,
            configs,
            pools: Mutex::new(HashMap::new()),
        }
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
        let ready = self.ready(app, manifest, dir)?;
        let handle = AppHandle::Php(handler::arc(ready.handler));
        self.pools
            .lock()
            .unwrap()
            .insert((app.clone(), manifest.version.clone()), ready.pool);
        Ok(handle)
    }

    fn ready(&self, app: &AppId, manifest: &ReleaseManifest, dir: &Path) -> Result<Ready, String> {
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
            app: app.to_string(),
            socket: PathBuf::new(),
            release_dir: release_dir.clone(),
            site_dir: config.site_dir.clone(),
            ini: section.ini.clone(),
            bindings: config.bindings.clone(),
        };
        let version = manifest.version.to_string();
        let dir = prepare_socket_dir(&self.run_dir, app.as_str(), &version)?;
        let pool = EmbeddedPhpPool::start(
            self.exe.clone(),
            spec,
            dir,
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
        Ok(Ready { pool, handler })
    }
}

impl AppRuntime for PhpRuntime {
    fn prepare(
        &self,
        app: &AppId,
        manifest: &ReleaseManifest,
        dir: &Path,
    ) -> Result<Prepared, String> {
        let ready = self.ready(app, manifest, dir)?;
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
        self.pools
            .lock()
            .unwrap()
            .insert((prepared.app.clone(), prepared.version.clone()), ready.pool);
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
