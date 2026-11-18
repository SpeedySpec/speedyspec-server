//! Several pods' managers in one process, over one shared store, state store, member store and election.

mod support;

use std::sync::Arc;
use std::time::Duration;

use speedyspec_server_app::{Cluster, ReleaseError, ReleaseManager};
use speedyspec_server_domain::{AppKind, StateStore};
use speedyspec_server_infra_memory::{MemoryLeadership, MemoryMembers, RecordingRuntime};
use support::{World, id, v};

/// A world with `blog` 1.0.0 and 2.0.0 published, and one election and member store for every pod.
struct Pods {
    world: World,
    election: MemoryLeadership,
    members: MemoryMembers,
}

impl Pods {
    fn new() -> Pods {
        let mut world = World::new();
        world.app("blog", AppKind::Php, 3);
        world.publish("blog", "1.0.0", "");
        world.publish("blog", "2.0.0", "");
        Pods {
            world,
            election: MemoryLeadership::new(),
            members: MemoryMembers::new(),
        }
    }

    fn pod(&self, name: &str) -> (ReleaseManager, Arc<RecordingRuntime>) {
        let runtime = Arc::new(RecordingRuntime::new());
        let cluster = Cluster::new(
            name,
            Arc::new(self.election.member(name)),
            Arc::new(self.members.clone()),
            Duration::from_millis(50),
            Duration::from_secs(2),
        );
        (
            self.world.manager_in(Arc::new(cluster), runtime.clone()),
            runtime,
        )
    }
}

#[test]
fn follower_refuses_mutations_naming_the_leader() {
    let pods = Pods::new();
    let (_a, _) = pods.pod("a");
    let (b, _) = pods.pod("b");
    pods.election.elect("a");
    let err = b.install(&id("blog"), Some(v("1.0.0"))).unwrap_err();
    assert!(
        matches!(err, ReleaseError::NotLeader { leader: Some(ref l) } if l == "a"),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        "this pod is not the leader; run release commands on a"
    );
    assert!(
        pods.world.source.downloads().is_empty(),
        "a follower downloads nothing"
    );
    for refused in [
        b.switch(&[(id("blog"), v("1.0.0"))]).unwrap_err(),
        b.rollback(&id("blog"), None).unwrap_err(),
        b.pin(&id("blog"), v("1.0.0")).unwrap_err(),
        b.unpin(&id("blog")).unwrap_err(),
        b.update(&id("blog")).unwrap_err(),
    ] {
        assert!(
            matches!(refused, ReleaseError::NotLeader { .. }),
            "{refused:?}"
        );
    }
    pods.election.expire();
    assert_eq!(
        b.pin(&id("blog"), v("1.0.0")).unwrap_err().to_string(),
        "there is no leader right now; try again shortly"
    );
}

#[test]
fn writes_carry_the_leader_epoch() {
    let pods = Pods::new();
    let (a, _) = pods.pod("a");
    let (b, _) = pods.pod("b");
    pods.election.elect("a");
    a.install(&id("blog"), Some(v("1.0.0"))).unwrap();
    assert_eq!(pods.world.saved().epoch, 1);
    pods.election.elect("b");
    b.refresh().unwrap();
    b.install(&id("blog"), Some(v("2.0.0"))).unwrap();
    let saved = pods.world.saved();
    assert_eq!(saved.epoch, 2);
    assert_eq!(
        saved.apps[&id("blog")].kept,
        vec![v("1.0.0"), v("2.0.0")],
        "b carried on from a's state"
    );
}

#[test]
fn leader_past_renew_deadline_refuses_the_next_write() {
    let pods = Pods::new();
    let (a, runtime) = pods.pod("a");
    pods.election.elect("a");
    a.install(&id("blog"), Some(v("1.0.0"))).unwrap();
    let before = pods.world.saved();
    runtime.set_prepare_delay(Duration::from_millis(300));
    let a = Arc::new(a);
    let switching = {
        let a = a.clone();
        std::thread::spawn(move || a.switch(&[(id("blog"), v("1.0.0"))]))
    };
    std::thread::sleep(Duration::from_millis(100));
    pods.election.expire();
    let err = switching.join().unwrap().unwrap_err();
    assert!(matches!(err, ReleaseError::NotLeader { .. }), "{err:?}");
    assert_eq!(
        pods.world.saved(),
        before,
        "nothing was written after the lease ran out"
    );
}

#[test]
fn older_epoch_state_is_ignored_by_refresh() {
    let pods = Pods::new();
    let (a, _) = pods.pod("a");
    let (b, _) = pods.pod("b");
    let (c, _) = pods.pod("c");
    pods.election.elect("a");
    a.install(&id("blog"), Some(v("1.0.0"))).unwrap();
    pods.election.elect("b");
    b.refresh().unwrap();
    b.pin(&id("blog"), v("1.0.0")).unwrap();
    assert!(c.refresh().unwrap(), "c picks up b's state");
    let current = c.status();
    assert_eq!(current.epoch, 2);

    let mut late = current.clone();
    late.epoch = 1;
    late.revision += 10;
    late.apps.get_mut(&id("blog")).unwrap().pinned = None;
    pods.world.state.save(&late).unwrap();
    assert!(!c.refresh().unwrap(), "an older epoch's write is ignored");
    assert_eq!(c.status(), current);
}
