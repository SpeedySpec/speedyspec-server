//! Starts each generation's health monitor over its php apps' probes.

use std::sync::Arc;

use speedyspec_server_gate::{
    GateCalls, HealthMonitor, HealthSettings, HealthTargets, MaintenanceSet,
};

use crate::config::Config;
use crate::php::PhpRuntime;
use crate::php::calls::FastCgiGateCalls;

/// The release manager, once managed mode has one: an app turning unhealthy may roll its runtime back.
pub type ManagerCell = Arc<std::sync::OnceLock<Arc<speedyspec_server_app::ReleaseManager>>>;

/// Probes every php app whose live release names health probes, on the current tokio runtime, and keeps their
/// health maintenance in `maintenance`. With a release manager, an app turning unhealthy is reported to it (a
/// runtime on trial rolls back; in a cluster the member report carries it to the leader), and trials past their
/// period are confirmed.
pub fn spawn(
    config: &Config,
    php: Arc<PhpRuntime>,
    maintenance: Arc<MaintenanceSet>,
    manager: ManagerCell,
) {
    let calls: Arc<dyn GateCalls> = Arc::new(FastCgiGateCalls::new(
        php.clone(),
        &config.mounts,
        config.health.timeout,
    ));
    let targets: HealthTargets = Arc::new(move || php.health_targets());
    let settings = HealthSettings {
        interval: config.health.interval,
        timeout: config.health.timeout,
        fail_after: config.health.fail_after,
        pass_after: config.health.pass_after,
    };
    let on_change = {
        let (manager, maintenance) = (manager.clone(), maintenance.clone());
        Arc::new(
            move |app: &speedyspec_server_domain::AppId, unhealthy: bool| {
                let Some(manager) = manager.get().cloned() else {
                    return;
                };
                let (app, maintenance) = (app.clone(), maintenance.clone());
                tokio::task::spawn_blocking(move || {
                    manager.set_unhealthy(
                        maintenance
                            .in_maintenance_by(speedyspec_server_gate::MaintenanceSource::Health),
                    );
                    if unhealthy && let Err(err) = manager.runtime_unhealthy(&app) {
                        tracing::error!(event = "runtime_rollback_failed", app = %app, error = %err);
                    }
                });
            },
        )
    };
    HealthMonitor::spawn(targets, calls, maintenance, settings, on_change);
    let every = config.health.interval;
    tokio::spawn(async move {
        let mut ticks = tokio::time::interval(every);
        loop {
            ticks.tick().await;
            if let Some(manager) = manager.get().cloned() {
                let _ = tokio::task::spawn_blocking(move || manager.confirm_runtime_trials()).await;
            }
        }
    });
}
