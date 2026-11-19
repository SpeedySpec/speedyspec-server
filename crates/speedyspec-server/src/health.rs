//! Starts each generation's health monitor over its php apps' probes.

use std::sync::Arc;

use speedyspec_server_gate::{
    GateCalls, HealthMonitor, HealthSettings, HealthTargets, MaintenanceSet,
};

use crate::config::Config;
use crate::php::PhpRuntime;
use crate::php::calls::FastCgiGateCalls;

/// Probes every php app whose live release names health probes, on the current tokio runtime, and keeps their
/// health maintenance in `maintenance`.
pub fn spawn(config: &Config, php: Arc<PhpRuntime>, maintenance: Arc<MaintenanceSet>) {
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
    HealthMonitor::spawn(targets, calls, maintenance, settings, Arc::new(|_, _| {}));
}
