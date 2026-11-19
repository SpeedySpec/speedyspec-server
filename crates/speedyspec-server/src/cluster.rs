//! Cluster wiring: the `Cluster` a config describes, the readiness probe, and the loops the active generation runs
//! (heartbeat; as leader, take-over, repair and the rollout; as follower, following `state.json`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use salvo::prelude::*;
use speedyspec_server_app::{Cluster, ReleaseManager};
use speedyspec_server_domain::LeaseTimings;
use speedyspec_server_infra_fs::{FileLeaseLeadership, FsClusterMembers};

use crate::config::{Config, Leadership};

/// How long a pod waits before asking its keeper again for a rollout turn that has not started.
const TURN_RETRY: Duration = Duration::from_secs(15);

/// How often a pod rewrites its member file.
pub const HEARTBEAT: Duration = Duration::from_secs(2);

/// The cluster `config` describes: a single host unless `[cluster]` names another leadership.
pub fn build(config: &Config) -> Result<Arc<Cluster>, String> {
    let Some(settings) = &config.cluster else {
        return Ok(Arc::new(Cluster::single("local")));
    };
    if settings.leadership == Leadership::Single {
        return Ok(Arc::new(Cluster::single(&settings.pod)));
    }
    let data_dir = config.data_dir.as_ref().ok_or("[cluster] needs data_dir")?;
    let timings = LeaseTimings {
        lease: settings.lease_duration,
        renew_deadline: settings.renew_deadline,
        retry: settings.retry_period,
    };
    let leadership: Arc<dyn speedyspec_server_domain::Leadership> = match settings.leadership {
        Leadership::File => {
            let dir = data_dir.join("cluster");
            std::fs::create_dir_all(&dir)
                .map_err(|err| format!("creating {}: {err}", dir.display()))?;
            Arc::new(FileLeaseLeadership::start(
                &dir.join("lease"),
                &settings.pod,
                timings,
            ))
        }
        Leadership::Kubernetes => {
            use speedyspec_server_infra_kubernetes::{
                KubeLeadership, LeaseSettings, pod_namespace,
            };
            let namespace = settings
                .lease_namespace
                .clone()
                .or_else(pod_namespace)
                .ok_or("cluster.lease_namespace is not set, and this is not a pod (no service account namespace)")?;
            Arc::new(KubeLeadership::in_cluster(LeaseSettings {
                name: settings.lease_name.clone(),
                namespace,
                pod: settings.pod.clone(),
                timings,
            })?)
        }
        Leadership::Single => unreachable!("handled above"),
    };
    Ok(Arc::new(
        Cluster::new(
            &settings.pod,
            leadership,
            Arc::new(FsClusterMembers::new(data_dir, &settings.pod)),
            settings.poll_interval,
            settings.prepare_timeout,
        )
        .with_turn_timeout(settings.turn_timeout),
    ))
}

/// What the readiness probe answers from.
#[derive(Default)]
pub struct Readiness {
    /// Set once the generation has told its keeper it is ready.
    pub started: AtomicBool,
    /// Managed mode: whether this pod serves the active set.
    pub manager: OnceLock<Arc<ReleaseManager>>,
    /// Every app's maintenance: an app failing its health probes makes this pod not ready.
    pub maintenance: OnceLock<Arc<speedyspec_server_gate::MaintenanceSet>>,
}

impl Readiness {
    /// Why this pod is not ready, or `None` when it is.
    pub fn not_ready(&self) -> Option<String> {
        if !self.started.load(Ordering::SeqCst) {
            return Some("starting".to_string());
        }
        if self
            .manager
            .get()
            .is_some_and(|manager| !manager.is_ready())
        {
            return Some("this pod could not prepare the active release set".to_string());
        }
        let unhealthy = self.maintenance.get().map_or_else(Vec::new, |set| {
            set.in_maintenance_by(speedyspec_server_gate::MaintenanceSource::Health)
        });
        if !unhealthy.is_empty() {
            let names: Vec<&str> = unhealthy.iter().map(|app| app.as_str()).collect();
            return Some(format!("unhealthy: {}", names.join(", ")));
        }
        None
    }
}

struct Livez;

#[async_trait]
impl Handler for Livez {
    async fn handle(&self, _: &mut Request, _: &mut Depot, res: &mut Response, _: &mut FlowCtrl) {
        res.render("ok\n");
    }
}

struct Readyz(Arc<Readiness>);

#[async_trait]
impl Handler for Readyz {
    async fn handle(&self, _: &mut Request, _: &mut Depot, res: &mut Response, _: &mut FlowCtrl) {
        match self.0.not_ready() {
            None => res.render("ready\n"),
            Some(reason) => {
                res.status_code(StatusCode::SERVICE_UNAVAILABLE);
                res.render(format!("not ready: {reason}\n"));
            }
        }
    }
}

/// The probe listener's router: `/livez` and `/readyz`, and 404 for everything else.
pub fn probe_router(readiness: Arc<Readiness>) -> Router {
    Router::new()
        .push(Router::with_path("livez").get(Livez))
        .push(Router::with_path("readyz").get(Readyz(readiness)))
}

/// Starts the heartbeat and the role loop for a managed generation in a cluster. Both pause while `paused` is set
/// (a server switch starting, or draining), so a draining generation never writes.
/// What the generation does when the rollout says it is this pod's turn: ask its keeper to switch binaries.
pub type OnTurn = Arc<dyn Fn(semver::Version, std::path::PathBuf) + Send + Sync>;

pub fn spawn_loops(manager: Arc<ReleaseManager>, paused: Arc<AtomicBool>, on_turn: OnTurn) {
    if manager.cluster().is_single() {
        return;
    }
    {
        let (manager, paused) = (manager.clone(), paused.clone());
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(HEARTBEAT).await;
                if !paused.load(Ordering::SeqCst) {
                    let manager = manager.clone();
                    let _ = tokio::task::spawn_blocking(move || manager.heartbeat()).await;
                }
            }
        });
    }
    tokio::spawn(async move {
        let mut was_leader = false;
        let mut requested: Option<(semver::Version, std::time::Instant)> = None;
        let interval = manager.cluster().poll_interval();
        loop {
            tokio::time::sleep(interval).await;
            if paused.load(Ordering::SeqCst) {
                continue;
            }
            let view = manager.cluster().leadership().view();
            if view.is_leader != was_leader {
                if view.is_leader {
                    tracing::info!(event = "leader_acquired", pod = %manager.cluster().pod(), epoch = view.epoch);
                } else {
                    tracing::warn!(event = "leader_lost", pod = %manager.cluster().pod(), leader = ?view.leader);
                }
            }
            was_leader = view.is_leader;
            let worker = manager.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let manager = worker;
                if view.is_leader {
                    let exe = std::env::current_exe().ok();
                    if let Err(err) = manager.lead(exe.as_deref()) {
                        tracing::warn!(event = "lead_step_failed", error = %err);
                    }
                } else if let Err(err) = manager.follow() {
                    tracing::warn!(event = "follow_failed", error = %err);
                }
            })
            .await;
            if let Some((version, binary)) = manager.my_turn() {
                // Asked again after a while: the keeper may have refused (another handoff still draining).
                let again = requested
                    .as_ref()
                    .is_none_or(|(asked, at)| *asked != version || at.elapsed() >= TURN_RETRY);
                if again {
                    tracing::info!(event = "rollout_turn", pod = %manager.cluster().pod(), version = %version);
                    requested = Some((version.clone(), std::time::Instant::now()));
                    on_turn(version, binary);
                }
            } else {
                requested = None;
            }
        }
    });
}

#[cfg(test)]
mod readiness_tests {
    use super::*;
    use speedyspec_server_domain::AppId;
    use speedyspec_server_gate::{MaintenanceSet, MaintenanceSource};

    #[test]
    fn an_unhealthy_app_makes_the_pod_not_ready() {
        let readiness = Readiness::default();
        readiness.started.store(true, Ordering::SeqCst);
        let set = Arc::new(MaintenanceSet::default());
        let _ = readiness.maintenance.set(set.clone());
        assert_eq!(readiness.not_ready(), None);
        let blog = AppId::parse("blog").unwrap();
        set.set(&blog, MaintenanceSource::Operator, true);
        assert_eq!(
            readiness.not_ready(),
            None,
            "only health maintenance makes a pod not ready"
        );
        set.set(&blog, MaintenanceSource::Health, true);
        assert_eq!(readiness.not_ready().as_deref(), Some("unhealthy: blog"));
    }
}
