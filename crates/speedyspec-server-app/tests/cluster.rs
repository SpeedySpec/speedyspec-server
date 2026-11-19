//! Several pods' managers in one process, over one shared store, state store, member store and election.

mod support;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use speedyspec_server_app::{Cluster, Liveness, ReleaseError, ReleaseManager, RolloutStep};
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

    /// A pod whose binary is `server` (another version than the world's).
    fn pod_running(
        &self,
        name: &str,
        server: speedyspec_server_domain::ReleaseManifest,
    ) -> (Arc<ReleaseManager>, Arc<RecordingRuntime>) {
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
            Arc::new(
                self.world
                    .manager_with_server(Arc::new(cluster), runtime.clone(), server),
            ),
            runtime,
        )
    }

    fn pod(&self, name: &str) -> (Arc<ReleaseManager>, Arc<RecordingRuntime>) {
        self.pod_with(name, |manager| manager)
    }

    /// As `pod`, with the manager built further (a migrator).
    fn pod_with(
        &self,
        name: &str,
        build: impl FnOnce(ReleaseManager) -> ReleaseManager,
    ) -> (Arc<ReleaseManager>, Arc<RecordingRuntime>) {
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
            Arc::new(build(
                self.world.manager_in(Arc::new(cluster), runtime.clone()),
            )),
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

/// Pods `a` (leader) and `b` with a kept server 4.0.0 and blog 1.0.0 active, 2.0.0 kept.
fn rollout_pods() -> (Pods, Vec<(Arc<ReleaseManager>, Arc<RecordingRuntime>)>) {
    let mut world = World::new();
    world.app("server", AppKind::Server, 3);
    world.app("blog", AppKind::Php, 3);
    world.publish("blog", "1.0.0", "");
    world.publish("blog", "2.0.0", "");
    world.publish_server("4.0.0", "server", "", support::TARGET);
    let pods = Pods::from(world);
    let all: Vec<_> = ["a", "b"].iter().map(|p| pods.pod(p)).collect();
    pods.election.elect("a");
    let (a, b) = (&all[0].0, &all[1].0);
    for (manager, version) in [(a, "4.0.0")] {
        manager.install(&id("server"), Some(v(version))).unwrap();
    }
    a.install(&id("blog"), Some(v("1.0.0"))).unwrap();
    a.install(&id("blog"), Some(v("2.0.0"))).unwrap();
    b.follow().unwrap();
    let switching = run_switch(a, "1.0.0");
    staged(&pods);
    b.follow().unwrap();
    finish(switching).unwrap();
    b.follow().unwrap();
    for (manager, _) in &all {
        manager.report_server(running(support::SERVER_VERSION));
    }
    (pods, all)
}

/// A pod's server state: running and confirmed on `version`.
fn running(version: &str) -> speedyspec_server_domain::PodServer {
    speedyspec_server_domain::PodServer {
        running: Some(v(version)),
        confirmed: Some(v(version)),
        ..Default::default()
    }
}

#[test]
fn rollout_goes_pod_by_pod_and_confirms() {
    let (pods, all) = rollout_pods();
    let (a, b) = (&all[0].0, &all[1].0);
    let upgrade = a
        .begin_server_switch(&[(id("server"), v("4.0.0")), (id("blog"), v("2.0.0"))])
        .unwrap();
    assert_eq!(
        upgrade.rollout,
        Some(vec!["b".to_string(), "a".to_string()])
    );
    let saved = pods.world.saved();
    assert_eq!(saved.server.trial, Some(v("4.0.0")));
    let rollout = saved.rollout.expect("a rollout");
    assert_eq!(rollout.previous, v(support::SERVER_VERSION));
    assert_eq!(
        saved.pending_group, None,
        "a cluster records the group in the rollout"
    );

    assert_eq!(
        a.advance_rollout().unwrap(),
        Some(RolloutStep::Turn("b".to_string()))
    );
    assert_eq!(
        pods.world.saved().rollout.unwrap().turn,
        Some("b".to_string())
    );
    assert_eq!(a.advance_rollout().unwrap(), Some(RolloutStep::Wait));
    b.report_server(running("4.0.0"));
    assert_eq!(
        a.advance_rollout().unwrap(),
        Some(RolloutStep::Turn("a".to_string()))
    );
    a.report_server(running("4.0.0"));
    assert_eq!(a.advance_rollout().unwrap(), Some(RolloutStep::Done));
    let saved = pods.world.saved();
    assert_eq!(saved.server.confirmed, Some(v("4.0.0")));
    assert_eq!(saved.server.trial, None);
    assert_eq!(saved.rollout, None);
    assert_eq!(
        saved.apps[&id("blog")].active,
        Some(v("2.0.0")),
        "the group's apps become active"
    );
    assert_eq!(a.advance_rollout().unwrap(), None);
}

#[test]
fn rollout_failure_marks_version_failed_and_keeps_group_apps() {
    let (pods, all) = rollout_pods();
    let (a, b) = (&all[0].0, &all[1].0);
    a.begin_server_switch(&[(id("server"), v("4.0.0")), (id("blog"), v("2.0.0"))])
        .unwrap();
    a.advance_rollout().unwrap();
    let mut failed = running(support::SERVER_VERSION);
    failed
        .failed
        .insert(v("4.0.0"), "exited during its trial".to_string());
    let rollout = pods.world.saved().rollout.unwrap().id;
    failed.failed_in.insert(v("4.0.0"), rollout);
    b.report_server(failed);
    assert_eq!(
        a.advance_rollout().unwrap(),
        Some(RolloutStep::Failed {
            pod: "b".to_string(),
            reason: "exited during its trial".to_string()
        })
    );
    assert!(pods.world.saved().rollout.unwrap().reverting);
    assert!(
        a.begin_server_switch(&[(id("server"), v("4.0.0"))])
            .is_err(),
        "one rollout at a time"
    );
    assert_eq!(a.advance_rollout().unwrap(), Some(RolloutStep::Reverted));
    let saved = pods.world.saved();
    assert_eq!(saved.rollout, None);
    assert_eq!(saved.server.trial, None);
    assert_eq!(
        saved.server.confirmed, None,
        "the confirmed server is unchanged"
    );
    assert!(saved.server.failed[&v("4.0.0")].contains("b: exited during its trial"));
    assert_eq!(
        saved.apps[&id("blog")].active,
        Some(v("1.0.0")),
        "the group's apps stay on their previous versions"
    );
}

/// Final review I2: a take-over that fails (an unreadable state.json) is tried again by the next leader step, and
/// `repair` does not run before it has succeeded (it would write this pod's stale state over the inherited one).
#[test]
fn failed_take_over_is_retried_and_repair_waits_for_it() {
    let (pods, all) = three();
    let switching = run_switch(&all[0].0, "2.0.0");
    let revision = staged(&pods);
    pods.election.expire();
    let _ = finish(switching);
    pods.election.elect("b");
    let before = pods.world.saved();
    pods.world.state.fail_next_load();
    assert!(
        all[1].0.lead(None).is_err(),
        "the take-over could not read the state"
    );
    assert_eq!(
        pods.world.saved(),
        before,
        "nothing was written before a successful take-over"
    );
    let leading = {
        let b = all[1].0.clone();
        std::thread::spawn(move || b.lead(None))
    };
    std::thread::sleep(Duration::from_millis(60));
    all[2].0.follow().unwrap();
    leading.join().unwrap().unwrap();
    let saved = pods.world.saved();
    assert_eq!(
        saved.staged, None,
        "the retried take-over finished revision {revision}"
    );
    assert_eq!(saved.apps[&id("blog")].active, Some(v("2.0.0")));
}

/// Final review I3: a new leader keeping its running binary does not write over the previous leader's writes it had
/// not picked up yet.
#[test]
fn keep_running_binary_keeps_writes_it_had_not_seen() {
    let (pods, all) = three();
    all[0].0.pin(&id("blog"), v("1.0.0")).unwrap();
    pods.election.elect("b");
    let exe = pods.world.dir.path().join("exe");
    std::fs::write(&exe, b"binary").unwrap();
    all[1].0.keep_running_binary(&exe).unwrap();
    assert_eq!(
        pods.world.saved().apps[&id("blog")].pinned,
        Some(v("1.0.0")),
        "a's pin survived b's first write"
    );
}

/// Final review I4: a recreated Lease starts its epoch over at 0; the new leader still writes an epoch above every
/// one written before, so followers follow it.
#[test]
fn recreated_lease_epoch_still_moves_followers() {
    let (pods, all) = three();
    pods.election.elect("a");
    all[0].0.take_over().unwrap();
    all[0].0.pin(&id("blog"), v("1.0.0")).unwrap();
    all[2].0.follow().unwrap();
    let old = pods.world.saved().epoch;
    assert!(old >= 2);
    pods.election.recreate();
    pods.election.elect("b");
    assert!(
        all[1].0.cluster().view().epoch < old,
        "the new Lease's epoch is lower"
    );
    all[1].0.take_over().unwrap();
    all[1].0.unpin(&id("blog")).unwrap();
    assert!(pods.world.saved().epoch > old);
    all[2].0.follow().unwrap();
    assert_eq!(
        all[2].0.status().apps[&id("blog")].pinned,
        None,
        "c follows the new leader"
    );
    assert!(
        !all[1].0.repair().unwrap(),
        "the leader's own state is not seen as stale"
    );
}

/// Final review I5: while member files cannot be read, the leader neither prunes (a follower may still serve the old
/// release) nor stops waiting at once.
#[test]
fn unreadable_members_block_pruning_and_waiting() {
    let mut world = World::new();
    world.app("blog", AppKind::Php, 1);
    for version in ["1.0.0", "2.0.0"] {
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
    pods.members.fail_reads(true);
    let switching = run_switch(a, "2.0.0");
    staged(&pods);
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        !switching.is_finished(),
        "the leader keeps waiting while it cannot see the members"
    );
    pods.advance(Duration::from_secs(61));
    finish(switching).unwrap();
    assert!(
        pods.world.release_dir("blog", "1.0.0").exists(),
        "nothing is pruned while it cannot tell what the followers serve"
    );
    pods.members.fail_reads(false);
}

/// Final review I6: a turn that never finishes (the pod never upgrades) fails the rollout after the turn timeout.
#[test]
fn stuck_turn_fails_after_turn_timeout() {
    let (pods, all) = rollout_pods();
    let a = &all[0].0;
    a.begin_server_switch(&[(id("server"), v("4.0.0"))])
        .unwrap();
    assert_eq!(
        a.advance_rollout().unwrap(),
        Some(RolloutStep::Turn("b".to_string()))
    );
    assert_eq!(a.advance_rollout().unwrap(), Some(RolloutStep::Wait));
    for _ in 0..70 {
        pods.advance(Duration::from_secs(9));
        for (manager, _) in &all {
            manager.heartbeat();
        }
    }
    let step = a.advance_rollout().unwrap();
    assert!(
        matches!(step, Some(RolloutStep::Failed { ref pod, ref reason }) if pod == "b" && reason.contains("did not finish its turn")),
        "{step:?}"
    );
}

/// Final review I8: a pod that starts on the cluster's confirmed version (it missed the rollout) reports it
/// confirmed, not on trial forever.
#[test]
fn pod_booting_on_cluster_confirmed_version_is_confirmed() {
    let (pods, all) = rollout_pods();
    let a = &all[0].0;
    a.begin_server_switch(&[(id("server"), v("4.0.0"))])
        .unwrap();
    a.advance_rollout().unwrap();
    all[1].0.report_server(running("4.0.0"));
    a.advance_rollout().unwrap();
    a.report_server(running("4.0.0"));
    assert_eq!(a.advance_rollout().unwrap(), Some(RolloutStep::Done));
    // A third pod whose member file still says 3.2.1 starts on the cluster's 4.0.0.
    let mut old = running(support::SERVER_VERSION);
    old.failed.clear();
    pods.members
        .write(&MemberReport {
            pod: "c".to_string(),
            heartbeat: 1,
            server: old,
            ..Default::default()
        })
        .unwrap();
    let mut world_server = support::server_manifest();
    world_server.version = v("4.0.0");
    let (c, _) = pods.pod_running("c", world_server);
    c.booted(&v("4.0.0"));
    let report = pods
        .members
        .read_all()
        .unwrap()
        .into_iter()
        .find(|r| r.pod == "c")
        .unwrap();
    assert_eq!(report.server.confirmed, Some(v("4.0.0")));
    assert_eq!(report.server.trial, None);
}

#[test]
fn operator_maintenance_reaches_every_member() {
    let pods = Pods::new();
    let (a, _) = pods.pod("a");
    let (b, _) = pods.pod("b");
    pods.election.elect("a");
    assert!(matches!(
        b.set_maintenance(&[id("blog")], true).unwrap_err(),
        ReleaseError::NotLeader { leader: Some(ref l) } if l == "a"
    ));
    a.set_maintenance(&[id("blog")], true).unwrap();
    b.follow().unwrap();
    assert!(
        b.status().maintenance[&id("blog")].operator,
        "the follower sees the leader's maintenance"
    );
    a.set_maintenance(&[id("blog")], false).unwrap();
    b.follow().unwrap();
    assert!(!b.status().maintenance.contains_key(&id("blog")));
}

/// Counts migrations across every pod, and fails them while `fail` is set.
#[derive(Default)]
struct Counting {
    calls: std::sync::atomic::AtomicUsize,
    fail: std::sync::atomic::AtomicBool,
}

impl speedyspec_server_app::Migrator for Counting {
    fn migrate(
        &self,
        _: &speedyspec_server_domain::Prepared,
        _: &str,
        _: Duration,
    ) -> Result<(), String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
            Err("500: boom".into())
        } else {
            Ok(())
        }
    }
}

/// Three pods with blog 1.0.0 active, 3.0.0 (which migrates) kept, every pod with the same counting migrator.
fn three_migrating() -> (
    Pods,
    Vec<(Arc<ReleaseManager>, Arc<RecordingRuntime>)>,
    Arc<Counting>,
) {
    let mut world = World::new();
    world.app("blog", AppKind::Php, 3);
    world.publish("blog", "1.0.0", "");
    world.publish_manifest(
        "blog",
        "3.0.0",
        r#"{"app":"blog","version":"3.0.0","kind":"php","php":{"switch":{"maintenance":"self","migrate":"/migrate.php"}}}"#,
    );
    let pods = Pods::from(world);
    let counting = Arc::new(Counting::default());
    let all: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|p| {
            let counting = counting.clone();
            pods.pod_with(p, move |m| m.with_migrator(counting))
        })
        .collect();
    pods.election.elect("a");
    all[0].0.install(&id("blog"), Some(v("1.0.0"))).unwrap();
    all[0].0.install(&id("blog"), Some(v("3.0.0"))).unwrap();
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
    (pods, all, counting)
}

#[test]
fn only_the_leader_migrates_once() {
    let (pods, all, counting) = three_migrating();
    let switching = run_switch(&all[0].0, "3.0.0");
    staged(&pods);
    for (manager, _) in &all[1..] {
        manager.follow().unwrap();
    }
    finish(switching).unwrap();
    for (manager, _) in &all[1..] {
        manager.follow().unwrap();
    }
    assert_eq!(
        counting.calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "one migration for the cluster"
    );
    for (_, runtime) in &all {
        assert_eq!(runtime.active("blog"), Some(v("3.0.0")));
    }
    assert!(!pods.world.saved().maintenance.contains_key(&id("blog")));
}

#[test]
fn failed_cluster_migration_leaves_everyone_on_the_old_release() {
    let (pods, all, counting) = three_migrating();
    counting
        .fail
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let switching = run_switch(&all[0].0, "3.0.0");
    staged(&pods);
    for (manager, _) in &all[1..] {
        manager.follow().unwrap();
    }
    let err = finish(switching).unwrap_err();
    assert!(matches!(err, ReleaseError::Migration { .. }), "{err}");
    for (manager, _) in &all[1..] {
        manager.follow().unwrap();
    }
    for (_, runtime) in &all {
        assert_eq!(runtime.active("blog"), Some(v("1.0.0")));
    }
    let saved = pods.world.saved();
    assert_eq!(saved.staged, None);
    assert!(
        saved.maintenance[&id("blog")].migration_failed.is_some(),
        "every replica keeps maintenance on"
    );
}

#[test]
fn one_members_unhealthy_report_rolls_every_member_back() {
    let mut world = World::new();
    world.app("rt", AppKind::Runtime, 3);
    world.app("blog", AppKind::Php, 3);
    for version in ["1.0.0", "1.1.0"] {
        world.publish_binary(
            "rt",
            version,
            &format!(r#"{{"app":"rt","version":"{version}","kind":"runtime","contracts":[1]}}"#),
            support::TARGET,
        );
    }
    let pods = Pods::from(world);
    let build = |m: ReleaseManager| m.with_php_runtimes([(id("blog"), id("rt"))].into());
    let all: Vec<_> = ["a", "b"].iter().map(|p| pods.pod_with(p, build)).collect();
    pods.election.elect("a");
    for version in ["1.0.0", "1.1.0"] {
        all[0].0.install(&id("rt"), Some(v(version))).unwrap();
    }
    // b reports in, so the leader waits for it to prepare each set.
    all[1].0.follow().unwrap();
    for target in ["1.0.0", "1.1.0"] {
        let leader = all[0].0.clone();
        let switching = std::thread::spawn(move || leader.switch(&[(id("rt"), v(target))]));
        staged(&pods);
        all[1].0.follow().unwrap();
        switching.join().unwrap().unwrap();
        all[1].0.follow().unwrap();
    }
    assert!(pods.world.saved().runtime_trials.contains_key(&id("rt")));
    // b's blog turns unhealthy: it says so in its member report, and the leader rolls the runtime back.
    all[1].0.set_unhealthy(vec![id("blog")]);
    let leader = all[0].0.clone();
    let checking = std::thread::spawn(move || leader.check_member_health());
    let deadline = Instant::now() + Duration::from_secs(5);
    while pods.world.saved().staged.is_none() && !checking.is_finished() {
        assert!(Instant::now() < deadline, "no rollback was staged");
        std::thread::sleep(Duration::from_millis(5));
    }
    all[1].0.follow().unwrap();
    checking.join().unwrap().unwrap();
    all[1].0.follow().unwrap();
    for (_, runtime) in &all {
        assert_eq!(runtime.active("rt"), Some(v("1.0.0")));
    }
    assert!(
        pods.world.saved().apps[&id("rt")]
            .failed
            .contains_key(&v("1.1.0"))
    );
}
