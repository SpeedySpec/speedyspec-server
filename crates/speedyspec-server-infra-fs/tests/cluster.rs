//! Member files and the one-host lease file.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use speedyspec_server_domain::{ClusterMembers, Leadership, LeaseTimings, MemberReport};
use speedyspec_server_infra_fs::{FileLeaseLeadership, FsClusterMembers};

fn report(pod: &str, heartbeat: u64) -> MemberReport {
    MemberReport {
        pod: pod.to_string(),
        heartbeat,
        ..Default::default()
    }
}

/// Collects every log line written while `body` runs.
fn captured<T>(body: impl FnOnce() -> T) -> (T, String) {
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let sink = Sink::default();
    let writer = sink.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(move || writer.clone())
        .finish();
    let out = tracing::subscriber::with_default(subscriber, body);
    let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    (out, text)
}

#[test]
fn member_write_is_atomic_and_read_all_lists_every_pod() {
    let dir = tempfile::tempdir().unwrap();
    let a = FsClusterMembers::new(dir.path(), "a");
    let b = FsClusterMembers::new(dir.path(), "b");
    b.write(&report("b", 1)).unwrap();
    a.write(&report("a", 1)).unwrap();
    a.write(&report("a", 2)).unwrap();
    let all = a.read_all().unwrap();
    assert_eq!(
        all.iter()
            .map(|r| (r.pod.as_str(), r.heartbeat))
            .collect::<Vec<_>>(),
        vec![("a", 2), ("b", 1)]
    );
    let files: Vec<String> = std::fs::read_dir(dir.path().join("cluster/members"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(
        files.iter().all(|f| f.ends_with(".json")),
        "no temp file is left behind: {files:?}"
    );
    assert!(
        a.write(&report("b", 9)).is_err(),
        "a pod writes only its own member file"
    );
}

#[test]
fn torn_member_file_keeps_last_good_copy() {
    let dir = tempfile::tempdir().unwrap();
    let a = FsClusterMembers::new(dir.path(), "a");
    let reader = FsClusterMembers::new(dir.path(), "reader");
    a.write(&report("a", 5)).unwrap();
    assert_eq!(reader.read_all().unwrap()[0].heartbeat, 5);
    let path = dir.path().join("cluster/members/a.json");
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
    let (reads, log) = captured(|| {
        (0..3)
            .map(|_| reader.read_all().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(
        reads.iter().all(|r| r.len() == 1 && r[0].heartbeat == 5),
        "{reads:?}"
    );
    assert_eq!(log.matches("member_unreadable").count(), 1, "{log}");
    a.write(&report("a", 6)).unwrap();
    assert_eq!(reader.read_all().unwrap()[0].heartbeat, 6);
}

fn timings() -> LeaseTimings {
    LeaseTimings {
        lease: Duration::from_millis(900),
        renew_deadline: Duration::from_millis(600),
        retry: Duration::from_millis(100),
    }
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
fn file_lease_one_holder_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lease");
    let a = FileLeaseLeadership::start(&path, "a", timings());
    let b = FileLeaseLeadership::start(&path, "b", timings());
    assert!(wait_until(Duration::from_secs(2), || a.view().is_leader
        || b.view().is_leader));
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        assert!(
            !(a.view().is_leader && b.view().is_leader),
            "two leaders at once"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(a.view().leader, b.view().leader);
}

#[test]
fn file_lease_passes_on_after_holder_stops_renewing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lease");
    let a = FileLeaseLeadership::start(&path, "a", timings());
    assert!(wait_until(Duration::from_secs(2), || a.view().is_leader));
    let first = a.view().epoch;
    let b = FileLeaseLeadership::start(&path, "b", timings());
    a.pause_renewals(true);
    let started = Instant::now();
    assert!(wait_until(Duration::from_secs(3), || b.view().is_leader));
    assert!(
        started.elapsed() <= timings().lease + timings().retry * 3,
        "{:?}",
        started.elapsed()
    );
    assert_eq!(b.view().epoch, first + 1);
}

#[test]
fn file_lease_view_turns_false_at_renew_deadline_without_a_renewal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lease");
    let a = FileLeaseLeadership::start(&path, "a", timings());
    assert!(wait_until(Duration::from_secs(2), || a.view().is_leader));
    a.pause_renewals(true);
    let paused = Instant::now();
    assert!(wait_until(Duration::from_secs(2), || !a.view().is_leader));
    let gone = paused.elapsed();
    assert!(
        gone <= timings().renew_deadline + timings().retry,
        "{gone:?}"
    );
    assert!(
        gone < timings().lease,
        "a leader stops before anyone else could take the lease"
    );
}

#[test]
fn step_down_releases_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lease");
    let a = FileLeaseLeadership::start(&path, "a", timings());
    assert!(wait_until(Duration::from_secs(2), || a.view().is_leader));
    let b = FileLeaseLeadership::start(&path, "b", timings());
    a.step_down();
    assert!(!a.view().is_leader);
    let started = Instant::now();
    assert!(wait_until(Duration::from_secs(2), || b.view().is_leader));
    assert!(
        started.elapsed() < timings().lease,
        "{:?}",
        started.elapsed()
    );
}
