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
use crate::runtime::StaticRuntime;

/// How often expired static fallbacks are dropped.
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// Builds the release manager over `data_dir`, warns about unsigned apps, and runs start-up recovery, which
/// activates every app's last active release into `live`.
pub fn start(
    config: &Config,
    live: Arc<LiveApps>,
    php: Arc<crate::php::PhpRuntime>,
    binaries: Arc<crate::php::runtime_kind::RuntimeBinaries>,
    maintenance: Arc<speedyspec_server_gate::MaintenanceSet>,
) -> Result<Arc<ReleaseManager>, String> {
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
    let runtimes: HashMap<AppKind, Arc<dyn AppRuntime>> = [(
        AppKind::Static,
        Arc::new(StaticRuntime::new(
            live.clone(),
            config.static_memory,
            config.static_fallback_grace,
        )) as Arc<dyn AppRuntime>,
    )]
    .into_iter()
    .chain(php_runtimes(php, binaries))
    .collect();
    let manager = ReleaseManager::new(
        policies,
        server_manifest(server),
        Arc::new(HttpSource::new()),
        Arc::new(verifier),
        Arc::new(FsReleaseStore::new(data_dir)),
        Arc::new(FsStateStore::new(data_dir, &config.run_dir)),
        runtimes,
        Arc::new(crate::binary::ProcessInspector),
        crate::binary::TARGET.to_string(),
        crate::cluster::build(config)?,
    )
    .map_err(|err| err.to_string())?
    .with_runtime_inspector(Arc::new(crate::binary::RuntimeInspector))
    .with_php_runtimes(config.php_runtimes())
    .with_migrator(Arc::new(crate::php::calls::PhpMigrator::new(
        &config.mounts,
    )))
    .with_maintenance(
        Arc::new(crate::php::calls::UpgradeMaintenance(maintenance)),
        config
            .gate
            .as_ref()
            .and_then(|gate| gate.maintenance_apps.clone())
            .unwrap_or_default(),
    )
    .with_migrate_timeout(config.migrate_timeout);

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

/// The server binary the keeper runs in managed mode: on first start the running executable is kept and
/// confirmed as the first server release; afterwards the confirmed binary in the data directory.
pub fn confirmed_binary(config: &Config) -> Result<std::path::PathBuf, String> {
    let data_dir = config
        .data_dir
        .as_ref()
        .ok_or("mode = \"managed\" needs data_dir")?;
    let server = config
        .apps
        .iter()
        .find(|(_, app)| app.kind == AppKind::Server)
        .map(|(id, _)| id.clone())
        .ok_or("no server app is configured")?;
    let exe = std::env::current_exe().map_err(|err| format!("finding this executable: {err}"))?;
    if let Some(cluster) = config
        .cluster
        .as_ref()
        .filter(|c| c.leadership != crate::config::Leadership::Single)
    {
        // A pod never writes the shared store on its own: it runs its own confirmed binary, else the cluster's,
        // else this executable (the leader keeps that as a release once it leads).
        use speedyspec_server_domain::{ClusterMembers, ReleaseStore, StateStore};
        let store = FsReleaseStore::new(data_dir);
        let binary = |version: &semver::Version| {
            Some(
                store
                    .release_dir(&server, version)
                    .join(speedyspec_server_app::SERVER_BINARY),
            )
            .filter(|path| path.exists())
        };
        let own = speedyspec_server_infra_fs::FsClusterMembers::new(data_dir, &cluster.pod)
            .read_all()
            .ok()
            .and_then(|reports| reports.into_iter().find(|r| r.pod == cluster.pod))
            .and_then(|report| report.server.confirmed);
        let shared_state = FsStateStore::new(data_dir, &config.run_dir).load().ok();
        let shared = shared_state
            .as_ref()
            .and_then(|state| state.server.confirmed.clone());
        // With no rollout running, every pod belongs on the cluster's confirmed binary: one that missed a rollout
        // catches up when it starts. During a rollout a pod keeps its own confirmed one.
        let rolling = shared_state
            .as_ref()
            .is_some_and(|state| state.rollout.is_some());
        if !rolling && let Some(path) = shared.as_ref().and_then(binary) {
            return Ok(path);
        }
        return Ok(own
            .as_ref()
            .and_then(binary)
            .or_else(|| shared.as_ref().and_then(binary))
            .unwrap_or(exe));
    }
    speedyspec_server_app::register_running_binary(
        &FsReleaseStore::new(data_dir),
        &FsStateStore::new(data_dir, &config.run_dir),
        &server,
        &crate::binary::own_version(),
        &exe,
    )
    .map_err(|err| err.to_string())
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
pub fn spawn_auto_update(
    manager: Arc<ReleaseManager>,
    apps: Vec<(AppId, Duration)>,
    paused: Arc<std::sync::atomic::AtomicBool>,
) {
    for (app, interval) in apps {
        let (manager, paused) = (manager.clone(), paused.clone());
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
                // Only the leader installs; a follower picks the result up from state.json.
                if !paused.load(std::sync::atomic::Ordering::SeqCst)
                    && manager.cluster().view().is_leader
                {
                    let _ = tokio::task::spawn_blocking(move || update_once(&manager, &id)).await;
                }
                tokio::time::sleep(interval).await;
            }
        });
    }
}

/// One check: install and switch to the candidate, if there is one and nothing changed during the download.
fn update_once(manager: &ReleaseManager, app: &AppId) {
    if let Err(err) = manager.update(app) {
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

/// PHP releases: worker pools. The runtime kind's releases move the php apps on them to new pools.
fn php_runtimes(
    php: Arc<crate::php::PhpRuntime>,
    binaries: Arc<crate::php::runtime_kind::RuntimeBinaries>,
) -> Vec<(AppKind, Arc<dyn AppRuntime>)> {
    let kind = Arc::new(crate::php::runtime_kind::RuntimeKindRuntime::new(
        php.clone(),
        binaries,
    ));
    vec![(AppKind::Php, php), (AppKind::Runtime, kind)]
}

/// Keeps the operator's and failed migrations' maintenance, as recorded in the state file, in `maintenance`: at
/// once, then every half second (a follower's state changes through its poll of the shared state).
pub fn spawn_maintenance_sync(
    manager: Arc<ReleaseManager>,
    maintenance: Arc<speedyspec_server_gate::MaintenanceSet>,
    apps: Vec<speedyspec_server_domain::AppId>,
) {
    use speedyspec_server_gate::MaintenanceSource;
    tokio::spawn(async move {
        let mut ticks = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            ticks.tick().await;
            let state = manager.status();
            for app in &apps {
                let record = state.maintenance.get(app).cloned().unwrap_or_default();
                maintenance.set(app, MaintenanceSource::Operator, record.operator);
                maintenance.set(
                    app,
                    MaintenanceSource::Upgrade,
                    record.migrating || record.migration_failed.is_some(),
                );
            }
        }
    });
}
