//! The Lease against a fake API server.

mod support {
    pub mod fake_api;
}

use std::time::{Duration, Instant};

use speedyspec_server_domain::{Leadership, LeaseTimings};
use speedyspec_server_infra_kubernetes::{KubeLeadership, LeaseSettings};
use support::fake_api::FakeApi;

fn timings() -> LeaseTimings {
    LeaseTimings {
        lease: Duration::from_millis(1500),
        renew_deadline: Duration::from_millis(1000),
        retry: Duration::from_millis(100),
    }
}

fn pod(api: &FakeApi, name: &str) -> KubeLeadership {
    let config = kube::Config::new(api.url.parse::<http::Uri>().unwrap());
    KubeLeadership::start(
        config,
        LeaseSettings {
            name: "speedyspec-server".to_string(),
            namespace: "web".to_string(),
            pod: name.to_string(),
            timings: timings(),
        },
    )
    .unwrap()
}

fn wait_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    done()
}

#[test]
fn acquires_missing_lease() {
    let api = FakeApi::start();
    let a = pod(&api, "web-0");
    assert!(wait_until(Duration::from_secs(3), || a.view().is_leader));
    let lease = api.lease().unwrap();
    assert_eq!(lease["spec"]["holderIdentity"], "web-0");
    assert_eq!(
        lease["spec"]["leaseDurationSeconds"], 2,
        "rounded up to whole seconds"
    );
    assert_eq!(a.view().epoch, 0);
    assert_eq!(a.view().leader.as_deref(), Some("web-0"));
}

#[test]
fn second_pod_waits_then_takes_over_after_lease_duration() {
    let api = FakeApi::start();
    let a = pod(&api, "web-0");
    assert!(wait_until(Duration::from_secs(3), || a.view().is_leader));
    let b = pod(&api, "web-1");
    std::thread::sleep(Duration::from_millis(500));
    assert!(!b.view().is_leader, "b waits while a renews");
    assert_eq!(b.view().leader.as_deref(), Some("web-0"));
    drop(a);
    let gone = Instant::now();
    assert!(wait_until(Duration::from_secs(5), || b.view().is_leader));
    assert!(
        gone.elapsed() >= Duration::from_millis(1000),
        "not before the lease ran out: {:?}",
        gone.elapsed()
    );
    assert_eq!(api.lease().unwrap()["spec"]["leaseTransitions"], 1);
    assert_eq!(b.view().epoch, 1);
}

#[test]
fn conflict_on_update_retries_without_claiming_leadership() {
    let api = FakeApi::start();
    let a = pod(&api, "web-0");
    assert!(wait_until(Duration::from_secs(3), || a.view().is_leader));
    drop(a);
    let b = pod(&api, "web-1");
    std::thread::sleep(Duration::from_millis(1600));
    api.conflict_next();
    let puts = api.puts();
    assert!(
        wait_until(Duration::from_secs(5), || api.puts() > puts + 1),
        "it tried again after the conflict"
    );
    assert!(wait_until(Duration::from_secs(3), || b.view().is_leader));
    assert_eq!(api.lease().unwrap()["spec"]["holderIdentity"], "web-1");
}

#[test]
fn renewal_failures_drop_leadership_at_renew_deadline() {
    let api = FakeApi::start();
    let a = pod(&api, "web-0");
    assert!(wait_until(Duration::from_secs(3), || a.view().is_leader));
    api.fail_updates(true);
    let failing = Instant::now();
    assert!(wait_until(Duration::from_secs(3), || !a.view().is_leader));
    let lost = failing.elapsed();
    assert!(
        lost <= timings().renew_deadline + timings().retry * 2,
        "{lost:?}"
    );
    assert!(
        lost < timings().lease,
        "a leader stops before anyone else could take the lease"
    );
}

#[test]
fn step_down_lets_another_pod_take_over_at_once() {
    let api = FakeApi::start();
    let a = pod(&api, "web-0");
    assert!(wait_until(Duration::from_secs(3), || a.view().is_leader));
    let b = pod(&api, "web-1");
    a.step_down();
    assert!(!a.view().is_leader);
    let started = Instant::now();
    assert!(wait_until(Duration::from_secs(3), || b.view().is_leader));
    assert!(
        started.elapsed() < timings().lease,
        "{:?}",
        started.elapsed()
    );
}
