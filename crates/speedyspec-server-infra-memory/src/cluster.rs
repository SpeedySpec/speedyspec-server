//! In-memory leadership and member stores, for tests of several managers in one process.

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use speedyspec_server_domain::{ClusterMembers, LeaderView, Leadership, MemberReport, StateError};

/// A single host: always the leader, epoch 0.
pub struct SingleLeadership {
    pod: String,
    /// Kept so `changed` receivers wait instead of seeing a closed channel.
    senders: Mutex<Vec<Sender<LeaderView>>>,
}

impl SingleLeadership {
    pub fn new(pod: impl Into<String>) -> SingleLeadership {
        SingleLeadership {
            pod: pod.into(),
            senders: Mutex::new(Vec::new()),
        }
    }
}

impl Leadership for SingleLeadership {
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
struct Election {
    leader: Option<String>,
    epoch: u64,
    watchers: Vec<(String, Sender<LeaderView>)>,
}

/// One election shared by several pods in one process. `member(pod)` is each pod's `Leadership`.
#[derive(Clone, Default)]
pub struct MemoryLeadership {
    election: Arc<Mutex<Election>>,
}

/// One pod's view of a [`MemoryLeadership`].
pub struct MemoryMember {
    pod: String,
    election: Arc<Mutex<Election>>,
}

fn view_of(election: &Election, pod: &str) -> LeaderView {
    LeaderView {
        is_leader: election.leader.as_deref() == Some(pod),
        epoch: election.epoch,
        leader: election.leader.clone(),
    }
}

impl MemoryLeadership {
    pub fn new() -> MemoryLeadership {
        MemoryLeadership::default()
    }

    pub fn member(&self, pod: impl Into<String>) -> MemoryMember {
        MemoryMember {
            pod: pod.into(),
            election: self.election.clone(),
        }
    }

    /// Makes `pod` the leader, with a new epoch.
    pub fn elect(&self, pod: &str) {
        self.change(|e| {
            e.leader = Some(pod.to_string());
            e.epoch += 1;
        });
    }

    /// Nobody leads (a lease that ran out); the epoch stays until the next election.
    pub fn expire(&self) {
        self.change(|e| e.leader = None);
    }

    fn change(&self, apply: impl FnOnce(&mut Election)) {
        let mut election = self.election.lock().unwrap_or_else(|p| p.into_inner());
        apply(&mut election);
        let views: Vec<(Sender<LeaderView>, LeaderView)> = election
            .watchers
            .iter()
            .map(|(pod, tx)| (tx.clone(), view_of(&election, pod)))
            .collect();
        for (tx, view) in views {
            let _ = tx.send(view);
        }
    }
}

impl Leadership for MemoryMember {
    fn view(&self) -> LeaderView {
        view_of(
            &self.election.lock().unwrap_or_else(|p| p.into_inner()),
            &self.pod,
        )
    }

    fn changed(&self) -> Receiver<LeaderView> {
        let (tx, rx) = channel();
        self.election
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .watchers
            .push((self.pod.clone(), tx));
        rx
    }

    fn step_down(&self) {
        let election = self.election.lock().unwrap_or_else(|p| p.into_inner());
        if election.leader.as_deref() == Some(self.pod.as_str()) {
            drop(election);
            MemoryLeadership {
                election: self.election.clone(),
            }
            .expire();
        }
    }
}

#[derive(Default)]
struct Members {
    reports: BTreeMap<String, MemberReport>,
    fail_next_read: bool,
}

/// Every pod's member report, shared in memory. Clones share the same reports.
#[derive(Clone, Default)]
pub struct MemoryMembers {
    members: Arc<Mutex<Members>>,
}

impl MemoryMembers {
    pub fn new() -> MemoryMembers {
        MemoryMembers::default()
    }

    /// The next `read_all` fails, as a torn or unreadable volume would.
    pub fn fail_next_read(&self) {
        self.lock().fail_next_read = true;
    }

    /// Changes a pod's stored report in place.
    pub fn tamper(&self, pod: &str, change: impl FnOnce(&mut MemberReport)) {
        if let Some(report) = self.lock().reports.get_mut(pod) {
            change(report);
        }
    }

    /// Removes a pod's report (as if its file were deleted).
    pub fn remove(&self, pod: &str) {
        self.lock().reports.remove(pod);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Members> {
        self.members.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl ClusterMembers for MemoryMembers {
    fn write(&self, report: &MemberReport) -> Result<(), StateError> {
        self.lock()
            .reports
            .insert(report.pod.clone(), report.clone());
        Ok(())
    }

    fn read_all(&self) -> Result<Vec<MemberReport>, StateError> {
        let mut members = self.lock();
        if std::mem::take(&mut members.fail_next_read) {
            return Err(StateError {
                path: "memory://members".into(),
                message: "test: read failed".to_string(),
            });
        }
        Ok(members.reports.values().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_leadership_always_leads() {
        let single = SingleLeadership::new("host");
        assert_eq!(
            single.view(),
            LeaderView {
                is_leader: true,
                epoch: 0,
                leader: Some("host".to_string())
            }
        );
        single.step_down();
        assert!(
            single.view().is_leader,
            "a single host cannot give leadership away"
        );
    }

    #[test]
    fn memory_leadership_elect_bumps_epoch_and_notifies() {
        let shared = MemoryLeadership::new();
        let a = shared.member("a");
        let b = shared.member("b");
        let (seen_a, seen_b) = (a.changed(), b.changed());
        shared.elect("a");
        shared.elect("b");
        assert_eq!(
            a.view(),
            LeaderView {
                is_leader: false,
                epoch: 2,
                leader: Some("b".to_string())
            }
        );
        assert_eq!(
            b.view(),
            LeaderView {
                is_leader: true,
                epoch: 2,
                leader: Some("b".to_string())
            }
        );
        let first: Vec<LeaderView> = seen_a.try_iter().collect();
        let second: Vec<LeaderView> = seen_b.try_iter().collect();
        assert_eq!(first.len(), 2);
        assert_eq!(
            first.iter().map(|v| v.epoch).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(
            first[0].is_leader && !first[1].is_leader,
            "a's own view of each change"
        );
        assert_eq!(
            second
                .iter()
                .map(|v| (v.epoch, v.is_leader))
                .collect::<Vec<_>>(),
            vec![(1, false), (2, true)]
        );
    }

    #[test]
    fn memory_leadership_expire_leaves_no_leader() {
        let shared = MemoryLeadership::new();
        let a = shared.member("a");
        shared.elect("a");
        shared.expire();
        assert_eq!(
            a.view(),
            LeaderView {
                is_leader: false,
                epoch: 1,
                leader: None
            }
        );
        shared.elect("a");
        assert_eq!(a.view().epoch, 2);
    }

    #[test]
    fn memory_members_read_every_pod_and_fail_on_demand() {
        let members = MemoryMembers::new();
        let report = |pod: &str| MemberReport {
            pod: pod.to_string(),
            heartbeat: 1,
            ..Default::default()
        };
        members.write(&report("b")).unwrap();
        members.write(&report("a")).unwrap();
        let pods: Vec<String> = members
            .read_all()
            .unwrap()
            .into_iter()
            .map(|r| r.pod)
            .collect();
        assert_eq!(pods, vec!["a", "b"]);
        members.fail_next_read();
        assert!(members.read_all().is_err());
        members.tamper("a", |r| r.heartbeat = 99);
        assert_eq!(members.read_all().unwrap()[0].heartbeat, 99);
    }
}
