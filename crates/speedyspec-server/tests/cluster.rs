//! Several pods on one data directory, each its own keeper, with the one-host lease file (`leadership = "file"`).
#![cfg(feature = "test-hooks")]

mod support;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use support::run::{Running, command, get, start_with};

/// One pod's config: its own run_dir and listener, the shared data_dir, and fast cluster timings.
fn pod_config(dir: &Path, pod: &str, extra: &str) -> PathBuf {
    let toml = format!(
        r#"mode = "managed"
data_dir = "{data}"
run_dir = "{run}"

[[listener]]
address = "127.0.0.1:0"

[app.server]
kind = "server"

[cluster]
leadership = "file"
pod = "{pod}"
probe = "127.0.0.1:0"
lease_duration = "3s"
renew_deadline = "2s"
retry_period = "500ms"
poll_interval = "300ms"
prepare_timeout = "10s"
{extra}
"#,
        data = dir.join("data").display(),
        run = dir.join(format!("run-{pod}")).display(),
    );
    let path = dir.join(format!("{pod}.toml"));
    std::fs::write(&path, toml).unwrap();
    path
}

fn wait_for(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    done()
}

/// The probe listener's address, from the keeper's `listening` event.
fn probe_address(keeper: &Running) -> SocketAddr {
    let mut found = None;
    assert!(
        wait_for(Duration::from_secs(10), || {
            found = keeper.log().iter().find_map(|line| {
                let value: serde_json::Value = serde_json::from_str(line).ok()?;
                (value["event"] == "listening" && value["role"] == "probe")
                    .then(|| value["address"].as_str()?.parse().ok())
                    .flatten()
            });
            found.is_some()
        }),
        "no probe listener in the log: {:?}",
        keeper.log()
    );
    found.unwrap()
}

fn status(config: &Path) -> String {
    command(&["status", "--config", config.to_str().unwrap()]).1
}

#[test]
fn probe_answers_livez_and_readyz() {
    let dir = tempfile::tempdir().unwrap();
    let config = pod_config(dir.path(), "a", "");
    let mut keeper = start_with(&config, &[]);
    keeper.address();
    let probe = probe_address(&keeper);
    assert!(wait_for(Duration::from_secs(10), || get(probe, "/livez").0 == 200));
    assert!(
        wait_for(Duration::from_secs(10), || get(probe, "/readyz").0 == 200),
        "the leader is ready: {:?}",
        get(probe, "/readyz")
    );
    assert_eq!(
        get(probe, "/").0,
        404,
        "the probe listener serves nothing else"
    );
    assert!(
        wait_for(Duration::from_secs(10), || status(&config)
            .contains("cluster\ta\tleader\ta")),
        "{}",
        status(&config)
    );
    assert!(
        status(&config).contains("member\ta\tlive\t"),
        "{}",
        status(&config)
    );
}
