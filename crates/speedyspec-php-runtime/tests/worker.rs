//! The runtime binary's own commands.

use std::process::Command;

use speedyspec_runtime_contract::RuntimeManifest;

const BIN: &str = env!("CARGO_BIN_EXE_speedyspec-php-runtime");

#[test]
fn manifest_prints_contract_majors() {
    let output = Command::new(BIN).arg("manifest").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest: RuntimeManifest = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(manifest.contracts, vec![1]);
    assert_eq!(manifest.version.to_string(), env!("CARGO_PKG_VERSION"));
    assert!(manifest.php.starts_with("8.5."), "{}", manifest.php);
}

#[test]
fn worker_refuses_another_major() {
    let spec = serde_json::json!({
        "contract": 9, "app": "a", "socket": "/nonexistent/s.sock", "release_dir": "/", "site_dir": null,
        "ini": {}, "bindings": [], "env": []
    });
    let output = Command::new(BIN)
        .args(["worker", "--spec", &spec.to_string()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("contract 9"));
}

mod support;

use speedyspec_runtime_contract::WorkerReport;

/// The worker's report for the request: the stderr record that decodes as one.
fn report(answer: &support::Answer) -> WorkerReport {
    answer
        .stderr
        .iter()
        .find_map(|record| WorkerReport::decode(record))
        .expect("a worker report")
}

#[tokio::test(flavor = "current_thread")]
async fn reported_wake_reaches_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let app = support::release(
        dir.path(),
        "app",
        "<?php \\SpeedySpec\\Runtime\\report_next_wake(1234); echo 'ok';",
    );
    let mut worker = support::spawn(dir.path(), &app, None, &[]);
    worker.ready().await;
    let answer = worker.get(&app.join("index.php"), "/").await;
    assert_eq!(answer.body(), "ok", "{}", answer.stdout);
    assert_eq!(report(&answer).next_cron, Some(1234));
}

#[tokio::test(flavor = "current_thread")]
async fn untouched_request_reports_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let app = support::release(
        dir.path(),
        "app",
        "<?php if (isset($_GET['wake'])) { \\SpeedySpec\\Runtime\\report_next_wake(99); } echo 'ok';",
    );
    let mut worker = support::spawn(dir.path(), &app, None, &[]);
    worker.ready().await;
    let first = worker.get(&app.join("index.php"), "/").await;
    assert_eq!(report(&first).next_cron, None);
}

#[tokio::test(flavor = "current_thread")]
async fn earliest_wake_wins() {
    let dir = tempfile::tempdir().unwrap();
    let app = support::release(
        dir.path(),
        "app",
        "<?php \\SpeedySpec\\Runtime\\report_next_wake(500); \\SpeedySpec\\Runtime\\report_next_wake(300);
         \\SpeedySpec\\Runtime\\report_next_wake(900); \\SpeedySpec\\Runtime\\report_next_wake(-5); echo 'ok';",
    );
    let mut worker = support::spawn(dir.path(), &app, None, &[]);
    worker.ready().await;
    let answer = worker.get(&app.join("index.php"), "/").await;
    // A time in the past means now: 0.
    assert_eq!(report(&answer).next_cron, Some(0));
    // The next request starts with an empty slot.
    let again = worker.get(&app.join("index.php"), "/").await;
    assert_eq!(report(&again).next_cron, Some(0));
}

#[tokio::test(flavor = "current_thread")]
async fn a_wake_never_leaks_into_the_next_request() {
    let dir = tempfile::tempdir().unwrap();
    let app = support::release(
        dir.path(),
        "app",
        "<?php \\SpeedySpec\\Runtime\\report_next_wake(99); echo 'woke';",
    );
    std::fs::write(app.join("plain.php"), "<?php echo 'plain';").unwrap();
    let mut worker = support::spawn(dir.path(), &app, None, &[]);
    worker.ready().await;
    let woke = worker.get(&app.join("index.php"), "/").await;
    assert_eq!(report(&woke).next_cron, Some(99));
    let plain = worker.get(&app.join("plain.php"), "/plain.php").await;
    assert_eq!(plain.body(), "plain", "{}", plain.stdout);
    assert_eq!(report(&plain).next_cron, None);
}
