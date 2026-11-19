//! Per-app health: each PHP app's `live` and `ready` probes, counted so one slow answer never flips maintenance.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use speedyspec_server_domain::{AppId, HealthSection};

use crate::calls::GateCalls;
use crate::maintenance::{MaintenanceSet, MaintenanceSource};

/// Counts an app's probe results: `fail_after` failures in a row turn it unhealthy, `pass_after` passes in a row turn
/// it healthy again. A result opposite to the current streak starts a new one.
#[derive(Clone, Debug)]
pub struct HealthCounter {
    fail_after: u32,
    pass_after: u32,
    unhealthy: bool,
    streak: u32,
}

impl HealthCounter {
    pub fn new(fail_after: u32, pass_after: u32) -> HealthCounter {
        HealthCounter {
            fail_after: fail_after.max(1),
            pass_after: pass_after.max(1),
            unhealthy: false,
            streak: 0,
        }
    }

    /// Counts one probe. `Some(true)` when the app turns unhealthy, `Some(false)` when it recovers.
    pub fn observe(&mut self, ok: bool) -> Option<bool> {
        if ok != self.unhealthy {
            // The result agrees with the current state: nothing building up.
            self.streak = 0;
            return None;
        }
        self.streak += 1;
        let needed = if self.unhealthy {
            self.pass_after
        } else {
            self.fail_after
        };
        if self.streak < needed {
            return None;
        }
        self.streak = 0;
        self.unhealthy = !self.unhealthy;
        Some(self.unhealthy)
    }
}

/// Which apps to probe, read at every round, so a switch to a release with other probes applies at once: each php
/// app with a live release that names its probes.
pub type HealthTargets = Arc<dyn Fn() -> Vec<(AppId, HealthSection)> + Send + Sync>;

/// What is told when an app turns unhealthy (`true`) or healthy again (`false`).
pub type OnHealthChange = Arc<dyn Fn(&AppId, bool) + Send + Sync>;

/// Probes every target every `interval` and keeps [`MaintenanceSource::Health`] in step.
pub struct HealthMonitor;

/// How the monitor probes.
#[derive(Clone, Copy, Debug)]
pub struct HealthSettings {
    pub interval: Duration,
    pub timeout: Duration,
    pub fail_after: u32,
    pub pass_after: u32,
}

impl HealthMonitor {
    /// Starts probing on the current tokio runtime.
    pub fn spawn(
        targets: HealthTargets,
        calls: Arc<dyn GateCalls>,
        set: Arc<MaintenanceSet>,
        settings: HealthSettings,
        on_change: OnHealthChange,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut counters: HashMap<AppId, HealthCounter> = HashMap::new();
            let mut ticks = tokio::time::interval(settings.interval);
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticks.tick().await;
                let targets = targets();
                counters.retain(|app, _| targets.iter().any(|(target, _)| target == app));
                for (app, section) in targets {
                    let ok = probe(&*calls, &app, &section, settings.timeout).await;
                    let counter = counters.entry(app.clone()).or_insert_with(|| {
                        HealthCounter::new(settings.fail_after, settings.pass_after)
                    });
                    if let Some(unhealthy) = counter.observe(ok) {
                        tracing::warn!(event = if unhealthy { "app_unhealthy" } else { "app_healthy" }, app = %app);
                        set.set(&app, MaintenanceSource::Health, unhealthy);
                        on_change(&app, unhealthy);
                    }
                }
            }
        })
    }
}

/// Both probes answer 200 within `timeout`.
async fn probe(
    calls: &dyn GateCalls,
    app: &AppId,
    section: &HealthSection,
    timeout: Duration,
) -> bool {
    for path in [&section.live, &section.ready] {
        match tokio::time::timeout(timeout, calls.probe(app, path)).await {
            Ok(Ok(200)) => {}
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_unhealthy_after_three_failures_and_back_after_three_passes() {
        let mut c = HealthCounter::new(3, 3);
        assert_eq!(
            [c.observe(false), c.observe(false), c.observe(false)],
            [None, None, Some(true)]
        );
        assert_eq!(
            [c.observe(true), c.observe(true), c.observe(true)],
            [None, None, Some(false)]
        );
    }

    #[test]
    fn alternating_results_never_flip() {
        let mut c = HealthCounter::new(3, 3);
        for i in 0..20 {
            assert_eq!(c.observe(i % 2 == 0), None);
        }
    }

    #[test]
    fn a_healthy_app_stays_quiet() {
        let mut c = HealthCounter::new(3, 3);
        for _ in 0..10 {
            assert_eq!(c.observe(true), None);
        }
    }
}
