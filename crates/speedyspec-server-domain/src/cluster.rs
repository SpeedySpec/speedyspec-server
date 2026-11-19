//! The contracts of running several pods on one shared data directory: who leads, and what each pod reports.

use std::collections::BTreeMap;

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{AppId, StateError};

/// Who leads, as this pod sees it right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderView {
    /// Whether this pod may write shared state at this moment.
    pub is_leader: bool,
    /// The Lease epoch (its transition count): it increases every time the leader changes.
    pub epoch: u64,
    /// The leading pod's name, if any pod leads.
    pub leader: Option<String>,
}

/// Leader election. Implementations renew in the background; `view` is computed at the call, so a leader whose renew
/// deadline has passed stops being the leader at once, before anyone else can take over.
pub trait Leadership: Send + Sync {
    fn view(&self) -> LeaderView;
    /// Every change of view from now on, in order.
    fn changed(&self) -> std::sync::mpsc::Receiver<LeaderView>;
    /// Gives up leadership now (on shutdown), so a successor need not wait out the lease.
    fn step_down(&self);
}

/// How a lease is held: how long it lasts, by when the holder must have renewed it, and how often it tries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaseTimings {
    pub lease: std::time::Duration,
    pub renew_deadline: std::time::Duration,
    pub retry: std::time::Duration,
}

/// One pod's own server binary state, in a cluster (a single host keeps it in `state.json`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PodServer {
    /// The version this pod's active generation runs.
    pub running: Option<Version>,
    /// The binary this pod falls back to.
    pub confirmed: Option<Version>,
    pub trial: Option<Version>,
    pub failed: BTreeMap<Version, String>,
    /// For each failed version, the rollout (its `id`) whose trial failed; an older rollout's failure does not count.
    pub failed_in: BTreeMap<Version, u64>,
}

/// What one pod tells the others, in `<data_dir>/cluster/members/<pod>.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MemberReport {
    pub pod: String,
    /// Increases on every write. Readers judge liveness by when they saw it change, never by a timestamp.
    pub heartbeat: u64,
    /// The `state.json` revision whose active set this pod serves.
    pub serving: u64,
    /// The staged revision this pod has prepared.
    pub prepared: Option<u64>,
    /// The staged revision this pod could not prepare, and why.
    pub prepare_failed: Option<(u64, String)>,
    /// Every release this pod serves or holds prepared; pruning keeps them.
    pub holds: Vec<(AppId, Version)>,
    /// What the readiness probe answers.
    pub ready: bool,
    /// The apps this pod holds in maintenance because their health probes fail.
    #[serde(default)]
    pub unhealthy: Vec<AppId>,
    pub server: PodServer,
}

/// The member files of every pod.
pub trait ClusterMembers: Send + Sync {
    /// Replaces this pod's report.
    fn write(&self, report: &MemberReport) -> Result<(), StateError>;
    /// Every pod's report, sorted by pod name.
    fn read_all(&self) -> Result<Vec<MemberReport>, StateError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_report_round_trips_and_tolerates_missing_fields() {
        let mut report = MemberReport {
            pod: "web-0".to_string(),
            heartbeat: 41,
            serving: 7,
            prepared: Some(8),
            prepare_failed: None,
            holds: vec![(AppId::parse("admin").unwrap(), Version::new(1, 0, 0))],
            ready: true,
            server: PodServer::default(),
        };
        report.server.running = Some(Version::new(0, 1, 0));
        report
            .server
            .failed
            .insert(Version::new(0, 2, 0), "trial failed".to_string());
        let json = serde_json::to_string(&report).unwrap();
        assert_eq!(serde_json::from_str::<MemberReport>(&json).unwrap(), report);

        let sparse: MemberReport = serde_json::from_str(r#"{"pod":"web-1","later":true}"#).unwrap();
        assert_eq!(sparse.pod, "web-1");
        assert_eq!(sparse.heartbeat, 0);
        assert!(!sparse.ready);
        assert_eq!(sparse.server, PodServer::default());
    }
}
