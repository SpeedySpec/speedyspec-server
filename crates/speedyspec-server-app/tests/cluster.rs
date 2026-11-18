//! Several pods' managers in one process, over one shared store, state store, member store and election.

mod support;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use speedyspec_server_app::{Cluster, Liveness, ReleaseError, ReleaseManager};
use speedyspec_server_domain::{AppKind, ClusterMembers, MemberReport, StateStore};
use speedyspec_server_infra_memory::{MemoryLeadership, MemoryMembers, RecordingRuntime};
use support::{World, id, v};

/// A world with `blog` 1.0.0 and 2.0.0 published, and one election and member store for every pod.
struct Pods {
    world: World,
    election: MemoryLeadership,
    members: MemoryMembers,
    /// The clock every pod's liveness reads; tests move it forward by hand.
    now: Arc<Mutex<Instant>>,
}

impl Pods {
    fn new() -> Pods {
        let mut world = World::new();
        world.app("blog", AppKind::Php, 3);
        world.publish("blog", "1.0.0", "");
        world.publish("blog", "2.0.0", "");
        Pods::from(world)
    }

    fn from(world: World) -> Pods {
        Pods {
            world,
            election: MemoryLeadership::new(),
            members: MemoryMembers::new(),
            now: Arc::new(Mutex::new(Instant::now())),
        }
    }

    fn advance(&self, by: Duration) {
        *self.now.lock().unwrap() += by;
    }

    fn pod(&self, name: &str) -> (Arc<ReleaseManager>, Arc<RecordingRuntime>) {
        let runtime = Arc::new(RecordingRuntime::new());
        let cluster = Cluster::new(
            name,
            Arc::new(self.election.member(name)),
            Arc::new(self.members.clone()),
            Duration::from_millis(20),
            Duration::from_secs(60),
        );
        let now = self.now.clone();
        let cluster = cluster.with_clock(Arc::new(move || *now.lock().unwrap()));
        (
            Arc::new(self.world.manager_in(Arc::new(cluster), runtime.clone())),
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
    b.take_over().unwrap();
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
    runtime.set_prepare_delay(Duration::from_millis(300));
    let switching = {
        let a = a.clone();
        std::thread::spawn(move || a.switch(&[(id("blog"), v("1.0.0"))]))
    };
    std::thread::sleep(Duration::from_millis(100));
    pods.election.expire();
    // Staging was written while `a` still led; nothing may follow it.
    let before = pods.world.saved();
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
    b.take_over().unwrap();
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

/// Three pods with blog 1.0.0 active everywhere, and `a` leading.
fn three() -> (Pods, Vec<(Arc<ReleaseManager>, Arc<RecordingRuntime>)>) {
    let pods = Pods::new();
    let all: Vec<_> = ["a", "b", "c"].iter().map(|p| pods.pod(p)).collect();
    pods.election.elect("a");
    all[0].0.install(&id("blog"), Some(v("1.0.0"))).unwrap();
    all[0].0.install(&id("blog"), Some(v("2.0.0"))).unwrap();
    for (manager, _) in &all[1..] {
        manager.follow().unwrap();
    }
    let switching = run_switch(&all[0].0, "1.0.0");
    staged(&pods);
    for (manager, _) in &all[1..] {
        manager.follow().unwrap();
    }
    finish(switching).unwrap();
    for (manager, _) in &all[1..] {
        manager.follow().unwrap();
    }
    for (_, runtime) in &all {
        assert_eq!(runtime.active("blog"), Some(v("1.0.0")));
    }
    (pods, all)
}

/// Runs `leader.switch(blog@version)` on another thread, since it waits for the other pods.
fn run_switch(
    leader: &Arc<ReleaseManager>,
    version: &str,
) -> std::thread::JoinHandle<Result<(), ReleaseError>> {
    let (leader, version) = (leader.clone(), v(version));
    std::thread::spawn(move || leader.switch(&[(id("blog"), version)]))
}

fn finish(handle: std::thread::JoinHandle<Result<(), ReleaseError>>) -> Result<(), ReleaseError> {
    handle.join().unwrap()
}

/// Waits (real time) until the leader has staged a set, and returns its revision.
fn staged(pods: &Pods) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(staged) = pods.world.saved().staged {
            return staged.revision;
        }
        assert!(Instant::now() < deadline, "nothing was staged");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn two_step_switch_lands_on_all_three() {
    let (pods, all) = three();
    let switching = run_switch(&all[0].0, "2.0.0");
    staged(&pods);
    for (manager, _) in &all[1..] {
        manager.follow().unwrap();
    }
    finish(switching).unwrap();
    for (manager, _) in &all[1..] {
        assert!(manager.follow().unwrap().ready);
    }
    for (_, runtime) in &all {
        assert_eq!(runtime.active("blog"), Some(v("2.0.0")));
    }
    let saved = pods.world.saved();
    assert_eq!(saved.staged, None);
    assert_eq!(saved.apps[&id("blog")].active, Some(v("2.0.0")));
}

#[test]
fn member_failing_prepare_goes_not_ready_and_others_switch() {
    let (pods, all) = three();
    all[2].1.fail_prepare("blog", "2.0.0", "out of memory");
    let switching = run_switch(&all[0].0, "2.0.0");
    staged(&pods);
    for (manager, _) in &all[1..] {
        manager.follow().unwrap();
    }
    finish(switching).unwrap();
    assert!(all[1].0.follow().unwrap().ready);
    let c = all[2].0.follow().unwrap();
    assert!(!c.ready, "c could not prepare the active set");
    assert_eq!(all[1].1.active("blog"), Some(v("2.0.0")));
    assert_eq!(
        all[2].1.active("blog"),
        Some(v("1.0.0")),
        "c keeps serving the old set"
    );
    let report = pods
        .members
        .read_all()
        .unwrap()
        .into_iter()
        .find(|r| r.pod == "c")
        .unwrap();
    assert!(!report.ready);

    let switching = run_switch(&all[0].0, "1.0.0");
    staged(&pods);
    for (manager, _) in &all[1..] {
        manager.follow().unwrap();
    }
    finish(switching).unwrap();
    assert!(
        all[2].0.follow().unwrap().ready,
        "c is ready again once a set it can prepare is active"
    );
}

#[test]
fn no_pod_prepares_abandons_and_marks_failed() {
    let (pods, all) = three();
    for (_, runtime) in &all {
        runtime.fail_prepare("blog", "2.0.0", "broken");
    }
    let switching = run_switch(&all[0].0, "2.0.0");
    staged(&pods);
    for (manager, _) in &all[1..] {
        manager.follow().unwrap();
    }
    let err = finish(switching).unwrap_err();
    assert!(matches!(err, ReleaseError::Prepare { .. }), "{err:?}");
    let saved = pods.world.saved();
    assert_eq!(saved.staged, None);
    assert_eq!(saved.apps[&id("blog")].active, Some(v("1.0.0")));
    assert!(saved.apps[&id("blog")].failed[&v("2.0.0")].contains("broken"));
}

#[test]
fn stale_member_is_not_waited_for() {
    let (pods, all) = three();
    let switching = run_switch(&all[0].0, "2.0.0");
    staged(&pods);
    all[1].0.follow().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(!switching.is_finished(), "a waits for c while c is live");
    pods.advance(Duration::from_secs(11));
    let started = Instant::now();
    finish(switching).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "a stopped waiting once c went stale"
    );
    assert_eq!(all[0].1.active("blog"), Some(v("2.0.0")));
}

#[test]
fn report_from_before_the_staged_revision_does_not_count() {
    let (pods, all) = three();
    let switching = run_switch(&all[0].0, "2.0.0");
    let revision = staged(&pods);
    all[1].0.follow().unwrap();
    pods.members.tamper("c", |r| {
        r.prepared = Some(revision - 1);
        r.heartbeat += 1;
    });
    std::thread::sleep(Duration::from_millis(150));
    assert!(!switching.is_finished(), "c's older report does not count");
    all[2].0.follow().unwrap();
    finish(switching).unwrap();
}

#[test]
fn prune_keeps_releases_live_members_hold() {
    let mut world = World::new();
    world.app("blog", AppKind::Php, 1);
    for version in ["1.0.0", "2.0.0", "3.0.0"] {
        world.publish("blog", version, "");
    }
    let pods = Pods::from(world);
    let all: Vec<_> = ["a", "b"].iter().map(|p| pods.pod(p)).collect();
    pods.election.elect("a");
    let (a, b) = (&all[0].0, &all[1].0);
    b.follow().unwrap();
    a.install(&id("blog"), Some(v("1.0.0"))).unwrap();
    let switching = run_switch(a, "1.0.0");
    staged(&pods);
    b.follow().unwrap();
    finish(switching).unwrap();
    b.follow().unwrap();
    a.install(&id("blog"), Some(v("2.0.0"))).unwrap();
    let switching = run_switch(a, "2.0.0");
    staged(&pods);
    b.follow().unwrap();
    finish(switching).unwrap();
    assert!(
        pods.world.release_dir("blog", "1.0.0").exists(),
        "b still serves 1.0.0, so it is kept although keep = 1"
    );
    b.follow().unwrap();
    a.install(&id("blog"), Some(v("3.0.0"))).unwrap();
    let switching = run_switch(a, "3.0.0");
    staged(&pods);
    b.follow().unwrap();
    finish(switching).unwrap();
    assert!(
        !pods.world.release_dir("blog", "1.0.0").exists(),
        "nobody holds 1.0.0 any more"
    );
}

#[test]
fn liveness_ignores_writer_clock() {
    let start = Instant::now();
    let mut liveness = Liveness::new();
    let report = |heartbeat| MemberReport {
        pod: "far".to_string(),
        heartbeat,
        ..Default::default()
    };
    assert_eq!(
        liveness.observe(&[report(1)], start),
        vec!["far"],
        "a pod seen for the first time is live"
    );
    for tick in 2..20u64 {
        let now = start + Duration::from_secs(2 * tick);
        assert_eq!(
            liveness.observe(&[report(tick)], now),
            vec!["far"],
            "live while its counter moves"
        );
    }
    let stopped = start + Duration::from_secs(38);
    assert_eq!(
        liveness.observe(&[report(19)], stopped + Duration::from_secs(9)),
        vec!["far"]
    );
    assert!(
        liveness
            .observe(&[report(19)], stopped + Duration::from_secs(11))
            .is_empty(),
        "stale 10 s after it stops"
    );
    assert_eq!(
        liveness.observe(&[report(20)], stopped + Duration::from_secs(12)),
        vec!["far"],
        "live again"
    );
}

/// Runs `manager.take_over()` on another thread, since it waits for the other pods like a switch does.
fn run_take_over(
    manager: &Arc<ReleaseManager>,
) -> std::thread::JoinHandle<Result<speedyspec_server_app::TakeOver, ReleaseError>> {
    let manager = manager.clone();
    std::thread::spawn(move || manager.take_over())
}

#[test]
fn leader_killed_mid_switch_new_leader_finishes() {
    let (pods, all) = three();
    let switching = run_switch(&all[0].0, "2.0.0");
    let revision = staged(&pods);
    std::thread::sleep(Duration::from_millis(60));
    pods.election.expire();
    let err = finish(switching).unwrap_err();
    assert!(
        matches!(err, ReleaseError::NotLeader { .. }),
        "the old leader gives up: {err:?}"
    );
    assert_eq!(
        pods.world.saved().staged.map(|s| s.revision),
        Some(revision),
        "the staged set is still there"
    );

    pods.election.elect("b");
    let taking = run_take_over(&all[1].0);
    std::thread::sleep(Duration::from_millis(60));
    all[2].0.follow().unwrap();
    let outcome = taking.join().unwrap().unwrap();
    assert_eq!(outcome.finished, Some(revision));
    assert_eq!(
        pods.world.saved().apps[&id("blog")].active,
        Some(v("2.0.0"))
    );
    all[2].0.follow().unwrap();
    assert_eq!(all[1].1.active("blog"), Some(v("2.0.0")));
    assert_eq!(all[2].1.active("blog"), Some(v("2.0.0")));
}

#[test]
fn new_leader_abandons_unverifiable_switch() {
    let (pods, all) = three();
    let switching = run_switch(&all[0].0, "2.0.0");
    let revision = staged(&pods);
    pods.election.expire();
    let _ = finish(switching);
    std::fs::remove_dir_all(pods.world.release_dir("blog", "2.0.0")).unwrap();
    pods.election.elect("b");
    let outcome = all[1].0.take_over().unwrap();
    let (abandoned, reason) = outcome.abandoned.expect("abandoned");
    assert_eq!(abandoned, revision);
    assert!(reason.contains("could not verify blog 2.0.0"), "{reason}");
    let saved = pods.world.saved();
    assert_eq!(saved.staged, None);
    assert_eq!(saved.apps[&id("blog")].active, Some(v("1.0.0")));
}

#[test]
fn deposed_leader_late_write_is_ignored() {
    let (pods, all) = three();
    let switching = run_switch(&all[0].0, "2.0.0");
    staged(&pods);
    pods.election.expire();
    let _ = finish(switching);
    pods.election.elect("b");
    let taking = run_take_over(&all[1].0);
    std::thread::sleep(Duration::from_millis(60));
    all[2].0.follow().unwrap();
    taking.join().unwrap().unwrap();
    let current = pods.world.saved();
    assert_eq!(current.epoch, 2);

    // `a`'s write from before it lost the lease lands late, stamped with its older epoch.
    let mut late = current.clone();
    late.epoch = 1;
    late.revision += 5;
    late.apps.get_mut(&id("blog")).unwrap().active = Some(v("1.0.0"));
    late.apps.get_mut(&id("blog")).unwrap().pinned = Some(v("1.0.0"));
    pods.world.state.save(&late).unwrap();
    all[2].0.follow().unwrap();
    assert_eq!(
        all[2].0.status().apps[&id("blog")].pinned,
        None,
        "c ignores the older epoch's write"
    );
    assert!(
        all[1].0.repair().unwrap(),
        "the leader rewrites the state the late write replaced"
    );
    let repaired = pods.world.saved();
    assert_eq!(repaired.epoch, 2);
    assert_eq!(repaired.apps[&id("blog")].active, Some(v("2.0.0")));
    assert!(!all[1].0.repair().unwrap());
    all[2].0.follow().unwrap();
    assert_eq!(
        all[2].1.active("blog"),
        Some(v("2.0.0")),
        "c serves the leader's set"
    );
}

#[test]
fn take_over_cleans_partials() {
    let (pods, all) = three();
    let partial = pods.world.dir.path().join("releases/blog/3.0.0.partial");
    std::fs::create_dir_all(&partial).unwrap();
    pods.election.elect("b");
    all[1].0.take_over().unwrap();
    assert!(!partial.exists());
}
