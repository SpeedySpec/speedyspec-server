//! Managed mode: the release manager over the data directory, start-up recovery, auto-update, and the sweep that
//! ends static fall-back grace periods.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use speedyspec_server_app::{AppPolicy, ReleaseManager, Signing};
use speedyspec_server_domain::{AppId, AppKind, AppRuntime};
use speedyspec_server_infra_fs::{FsReleaseStore, FsStateStore};
use speedyspec_server_infra_http::HttpSource;
use speedyspec_server_infra_minisign::MinisignVerifier;
use speedyspec_server_presentation::LiveApps;

use crate::boot::server_manifest;
use crate::config::{Config, UpdatePolicy};
use crate::runtime::{PhpPlaceholderRuntime, StaticRuntime};

/// How often expired static fallbacks are dropped.
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// Builds the release manager over `data_dir`, warns about unsigned apps, and runs start-up recovery, which
/// activates every app's last active release into `live`.
pub fn start(config: &Config, live: Arc<LiveApps>) -> Result<Arc<ReleaseManager>, String> {
    let data_dir = config
        .data_dir
        .as_ref()
        .ok_or("mode = \"managed\" needs data_dir")?;
    std::fs::create_dir_all(data_dir.join("releases"))
        .map_err(|err| format!("creating {}: {err}", data_dir.join("releases").display()))?;

    let mut policies = BTreeMap::new();
    let mut keys = BTreeMap::new();
    let mut server = None;
    for (app, settings) in &config.apps {
        let signing = match (settings.keys.is_empty(), settings.allow_unsigned) {
            (false, _) => Signing::Verify,
            (true, true) => Signing::AllowUnsigned,
            (true, false) => Signing::NoKeys,
        };
        if signing == Signing::AllowUnsigned && settings.kind != AppKind::Server {
            tracing::warn!(event = "unsigned", app = %app, "allow_unsigned = true: releases are installed without signature checks");
        }
        if !settings.keys.is_empty() {
            keys.insert(app.clone(), settings.keys.clone());
        }
        if settings.kind == AppKind::Server {
            server = Some(app.clone());
        }
        policies.insert(
            app.clone(),
            AppPolicy {
                kind: settings.kind,
                source: settings.source.clone(),
                keep: settings.keep,
                prerelease: settings.prerelease,
                signing,
            },
        );
    }
    let server = server.ok_or("no server app is configured")?;
    let verifier = MinisignVerifier::new(keys).map_err(|err| err.0)?;
    let runtimes: HashMap<AppKind, Arc<dyn AppRuntime>> = [
        (
            AppKind::Static,
            Arc::new(StaticRuntime::new(
                live.clone(),
                config.static_memory,
                config.static_fallback_grace,
            )) as Arc<dyn AppRuntime>,
        ),
        (
            AppKind::Php,
            Arc::new(PhpPlaceholderRuntime::new(live)) as Arc<dyn AppRuntime>,
        ),
    ]
    .into_iter()
    .collect();
    let manager = ReleaseManager::new(
        policies,
        server_manifest(server),
        Arc::new(HttpSource::new()),
        Arc::new(verifier),
        Arc::new(FsReleaseStore::new(data_dir)),
        Arc::new(FsStateStore::new(data_dir, &config.run_dir)),
        runtimes,
    )
    .map_err(|err| err.to_string())?;

    let report = manager.boot();
    tracing::info!(
        event = "boot",
        cleaned = report.cleaned.len(),
        fallbacks = report.fallbacks.len(),
        not_installed = ?report.not_installed.iter().map(ToString::to_string).collect::<Vec<_>>(),
        incompatible = report.incompatible.len(),
    );

    Ok(Arc::new(manager))
}

/// The `auto` apps (never the server) and their check intervals.
pub fn auto_apps(config: &Config) -> Vec<(AppId, Duration)> {
    config
        .apps
        .iter()
        .filter(|(_, app)| app.update == UpdatePolicy::Auto && app.kind != AppKind::Server)
        .map(|(id, app)| (id.clone(), app.check_interval))
        .collect()
}

/// Spawns one auto-update loop per app: right away for an app with nothing active, otherwise after one interval,
/// then every interval. Failures are logged and never stop the loop.
pub fn spawn_auto_update(manager: Arc<ReleaseManager>, apps: Vec<(AppId, Duration)>) {
    for (app, interval) in apps {
        let manager = manager.clone();
        tokio::spawn(async move {
            let installed = manager
                .status()
                .apps
                .get(&app)
                .is_some_and(|state| state.active.is_some());
            if installed {
                tokio::time::sleep(interval).await;
            }
            loop {
                let (manager, id) = (manager.clone(), app.clone());
                let _ = tokio::task::spawn_blocking(move || update_once(&manager, &id)).await;
                tokio::time::sleep(interval).await;
            }
        });
    }
}

/// One check: install and switch to the candidate, if there is one.
fn update_once(manager: &ReleaseManager, app: &AppId) {
    let result = manager.check(app).and_then(|candidate| match candidate {
        None => Ok(()),
        Some(version) => {
            let version = manager.install(app, Some(version))?;
            manager.switch(&[(app.clone(), version)])
        }
    });
    if let Err(err) = result {
        tracing::warn!(event = "auto_update_failed", app = %app, error = %err);
    }
}

/// Drops expired static fallbacks every minute.
pub fn spawn_fallback_sweep(live: Arc<LiveApps>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(SWEEP_INTERVAL).await;
            live.drop_expired_fallbacks(Instant::now());
        }
    });
}
