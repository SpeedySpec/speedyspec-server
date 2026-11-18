//! What a release manager knows about the other pods: who leads, where the member reports are, and how long to wait.
//! A single host is a cluster of one that always leads.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use speedyspec_server_domain::{ClusterMembers, LeaderView, Leadership, MemberReport, StateError};

/// The clock liveness is judged by. `Instant::now` in production; tests pass their own.
pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// This pod's place in the cluster.
pub struct Cluster {
    pub(crate) pod: String,
    pub(crate) leadership: Arc<dyn Leadership>,
    pub(crate) members: Arc<dyn ClusterMembers>,
    pub(crate) poll_interval: Duration,
    pub(crate) prepare_timeout: Duration,
    pub(crate) clock: Clock,
    /// Whether this is a single host (`leadership = "single"`).
    pub(crate) single: bool,
    /// The newest epoch this pod has seen in `state.json`.
    pub(crate) seen_epoch: AtomicU64,
    pub(crate) liveness: Mutex<Liveness>,
}

/// A pod counts as stale once its heartbeat counter has not changed for this long, as this pod saw it.
pub const STALE_AFTER: Duration = Duration::from_secs(10);

/// Which pods are live, judged only by when *this* pod saw each one's heartbeat counter change. The writer's clock is
/// never read, so clock skew between pods does not matter.
#[derive(Default)]
pub struct Liveness {
    seen: BTreeMap<String, (u64, Instant)>,
    stale: BTreeSet<String>,
}

impl Liveness {
    pub fn new() -> Liveness {
        Liveness::default()
    }

    /// The live pods among `reports`, sorted by name. A pod seen for the first time is live.
    pub fn observe(&mut self, reports: &[MemberReport], now: Instant) -> Vec<String> {
        let mut live = Vec::new();
        for report in reports {
            let entry = self
                .seen
                .entry(report.pod.clone())
                .or_insert((report.heartbeat, now));
            if entry.0 != report.heartbeat {
                *entry = (report.heartbeat, now);
            }
            if now.saturating_duration_since(entry.1) < STALE_AFTER {
                if self.stale.remove(&report.pod) {
                    tracing::info!(event = "member_live", pod = %report.pod);
                }
                live.push(report.pod.clone());
            } else if self.stale.insert(report.pod.clone()) {
                tracing::warn!(event = "member_stale", pod = %report.pod);
            }
        }
        live.sort();
        live
    }
}

impl Cluster {
    pub fn new(
        pod: &str,
        leadership: Arc<dyn Leadership>,
        members: Arc<dyn ClusterMembers>,
        poll_interval: Duration,
        prepare_timeout: Duration,
    ) -> Cluster {
        Cluster {
            pod: pod.to_string(),
            leadership,
            members,
            poll_interval,
            prepare_timeout,
            clock: Arc::new(Instant::now),
            single: false,
            seen_epoch: AtomicU64::new(0),
            liveness: Mutex::new(Liveness::new()),
        }
    }

    /// A single host: always the leader, epoch 0, and its one member report kept in memory (no member file).
    pub fn single(pod: &str) -> Cluster {
        let mut cluster = Cluster::new(
            pod,
            Arc::new(AlwaysLeader {
                pod: pod.to_string(),
                senders: Mutex::new(Vec::new()),
            }),
            Arc::new(OneMember::default()),
            Duration::from_secs(2),
            Duration::from_secs(60),
        );
        cluster.single = true;
        cluster
    }

    /// Replaces the clock liveness is judged by.
    pub fn with_clock(mut self, clock: Clock) -> Cluster {
        self.clock = clock;
        self
    }

    pub fn pod(&self) -> &str {
        &self.pod
    }

    pub fn is_single(&self) -> bool {
        self.single
    }

    pub fn view(&self) -> LeaderView {
        self.leadership.view()
    }

    pub fn leadership(&self) -> &Arc<dyn Leadership> {
        &self.leadership
    }

    pub fn members(&self) -> &Arc<dyn ClusterMembers> {
        &self.members
    }

    pub fn poll_interval(&self) -> Duration {
        self.poll_interval
    }

    pub fn prepare_timeout(&self) -> Duration {
        self.prepare_timeout
    }

    pub(crate) fn now(&self) -> Instant {
        (self.clock)()
    }

    /// Every member report, and which pods are live now. This pod always counts as live.
    pub(crate) fn live_members(&self) -> (Vec<MemberReport>, Vec<String>) {
        let reports = match self.members.read_all() {
            Ok(reports) => reports,
            Err(err) => {
                tracing::warn!(event = "members_unreadable", error = %err);
                Vec::new()
            }
        };
        let mut live = self
            .liveness
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .observe(&reports, self.now());
        if !live.contains(&self.pod) {
            live.push(self.pod.clone());
            live.sort();
        }
        (reports, live)
    }

    /// Records an epoch seen in `state.json`; returns false when it is older than one seen before.
    pub(crate) fn observe_epoch(&self, epoch: u64) -> bool {
        let seen = self.seen_epoch.fetch_max(epoch, Ordering::SeqCst);
        epoch >= seen
    }
}

struct AlwaysLeader {
    pod: String,
    senders: Mutex<Vec<Sender<LeaderView>>>,
}

impl Leadership for AlwaysLeader {
    fn view(&self) -> LeaderView {
        LeaderView {
            is_leader: true,
            epoch: 0,
            leader: Some(self.pod.clone()),
        }
    }

    fn changed(&self) -> Receiver<LeaderView> {
        let (tx, rx) = channel();
        self.senders
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(tx);
        rx
    }

    fn step_down(&self) {}
}

#[derive(Default)]
struct OneMember(Mutex<Option<MemberReport>>);

impl ClusterMembers for OneMember {
    fn write(&self, report: &MemberReport) -> Result<(), StateError> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(report.clone());
        Ok(())
    }

    fn read_all(&self) -> Result<Vec<MemberReport>, StateError> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .into_iter()
            .collect())
    }
}
