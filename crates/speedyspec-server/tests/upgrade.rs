//! Server upgrades by socket handoff: the trial, fall-back, group switches and rollback.
#![cfg(feature = "test-hooks")]

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use support::release_host::ReleaseHost;
use support::run::{BIN, Running, command, get, poll, start_with};

const BASE: &str = env!("CARGO_PKG_VERSION");

struct World {
    dir: tempfile::TempDir,
    host: ReleaseHost,
    config: PathBuf,
    keeper: Running,
    addr: std::net::SocketAddr,
}

impl World {
    fn cmd(&self, args: &[&str]) -> (i32, String, String) {
        let mut all: Vec<&str> = args[..args.len().min(2)].to_vec();
        all.push("--config");
        all.push(self.config.to_str().unwrap());
        all.extend_from_slice(&args[args.len().min(2)..]);
        command(&all)
    }

    fn ok(&self, args: &[&str]) -> String {
        let (code, stdout, stderr) = self.cmd(args);
        assert_eq!(code, 0, "{args:?}: {stderr}\nlog: {:?}", self.keeper.log());
        stdout
    }

    /// (pid, version, "trial" | "confirmed") from the `generation` line of `status`.
    fn generation(&self) -> (u32, String, String) {
        let out = self.ok(&["status"]);
        let line = out
            .lines()
            .find(|l| l.starts_with("generation\t"))
            .unwrap_or_else(|| panic!("{out}"));
        let parts: Vec<&str> = line.split('\t').collect();
        (
            parts[1].parse().unwrap(),
            parts[2].to_string(),
            parts[3].to_string(),
        )
    }

    fn state(&self) -> serde_json::Value {
        serde_json::from_str(
            &std::fs::read_to_string(self.dir.path().join("data/state.json")).unwrap(),
        )
        .unwrap()
    }
}

fn world(env: &[(&str, &str)], handoff: &str) -> World {
    let dir = tempfile::tempdir().unwrap();
    let host = ReleaseHost::start();
    host.publish_static("admin", "1.0.0");
    let toml = format!(
        r#"mode = "managed"
data_dir = "{data}"
run_dir = "{run}"

[handoff]
{handoff}

[[listener]]
address = "127.0.0.1:0"

[app.server]
kind = "server"
source = "{server_source}"
keys = ["{key}"]

[app.admin]
kind = "static"
source = "{admin_source}"
keys = ["{key}"]
update = "manual"

[[mount]]
path = "/"
app = "admin"
"#,
        data = dir.path().join("data").display(),
        run = dir.path().join("run").display(),
        server_source = host.url("/server/index.json"),
        admin_source = host.url("/admin/index.json"),
        key = host.public_key(),
    );
    let config = dir.path().join("server.toml");
    std::fs::write(&config, toml).unwrap();
    let mut all = vec![("SPEEDYSPEC_TEST_VERSION_FROM_DIR", "1")];
    all.extend_from_slice(env);
    let mut keeper = start_with(&config, &all);
    let addr = keeper.address();
    let socket = dir.path().join("run/keeper.sock");
    assert!(
        wait_for(Duration::from_secs(10), || socket.exists()),
        "no keeper.sock"
    );
    let w = World {
        dir,
        host,
        config,
        keeper,
        addr,
    };
    w.ok(&["release", "install", "admin", "1.0.0"]);
    w.ok(&["release", "switch", "admin@1.0.0"]);
    assert_eq!(get(addr, "/").1, "admin-1.0.0");
    w
}

const FAST: &str = "trial_period = \"3s\"\nready_timeout = \"5s\"\ndrain_timeout = \"5s\"";

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .unwrap()
        .success()
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

fn publish_and_install(w: &World, version: &str) {
    w.host.publish_server(version, Path::new(BIN));
    w.ok(&["release", "install", "server", version]);
}

#[test]
fn upgrade_switches_generation_and_confirms() {
    let w = world(&[], FAST);
    let (old_pid, version, _) = w.generation();
    assert_eq!(version, BASE);
    publish_and_install(&w, "0.2.0");
    assert!(
        w.ok(&["release", "switch", "server@0.2.0"])
            .starts_with("switched server 0.2.0 (on trial")
    );
    let (new_pid, version, trial) = w.generation();
    assert_ne!(new_pid, old_pid);
    assert_eq!((version.as_str(), trial.as_str()), ("0.2.0", "trial"));
    assert_eq!(get(w.addr, "/").1, "admin-1.0.0");
    assert!(wait_for(Duration::from_secs(8), || w.generation().2 == "confirmed"));
    assert_eq!(w.state()["server"]["confirmed"], "0.2.0");
    assert!(
        wait_for(Duration::from_secs(8), || !alive(old_pid)),
        "the old generation exits after the trial"
    );
}

#[test]
fn failed_trial_resumes_old_generation() {
    let w = world(
        &[
            ("SPEEDYSPEC_TEST_EXIT_VERSION", "0.2.0"),
            ("SPEEDYSPEC_TEST_EXIT_AFTER_READY_MS", "1000"),
        ],
        FAST,
    );
    let (old_pid, _, _) = w.generation();
    publish_and_install(&w, "0.2.0");
    w.ok(&["release", "switch", "server@0.2.0"]);
    let mut failures = 0;
    let resumed = wait_for(Duration::from_secs(8), || {
        if get(w.addr, "/").0 != 200 {
            failures += 1;
        }
        w.generation().0 == old_pid
    });
    assert!(resumed, "log: {:?}", w.keeper.log());
    assert_eq!(failures, 0, "requests failed during the failed trial");
    assert!(w.state()["server"]["failed"]["0.2.0"].is_string());
    assert_eq!(w.state()["server"]["trial"], serde_json::Value::Null);
}

#[test]
fn failed_trial_after_old_exited_starts_confirmed_binary() {
    let w = world(
        &[
            ("SPEEDYSPEC_TEST_EXIT_VERSION", "0.2.0"),
            ("SPEEDYSPEC_TEST_EXIT_AFTER_READY_MS", "2000"),
        ],
        FAST,
    );
    let (old_pid, _, _) = w.generation();
    publish_and_install(&w, "0.2.0");
    w.ok(&["release", "switch", "server@0.2.0"]);
    assert!(
        Command::new("kill")
            .arg("-KILL")
            .arg(old_pid.to_string())
            .status()
            .unwrap()
            .success()
    );
    assert!(wait_for(Duration::from_secs(10), || {
        let (pid, version, _) = w.generation();
        pid != old_pid && version == BASE
    }));
    assert_eq!(
        poll(w.addr, "/", Duration::from_secs(5), |s, _| s == 200).1,
        "admin-1.0.0"
    );
    assert!(w.state()["server"]["failed"]["0.2.0"].is_string());
}

#[test]
fn never_ready_binary_is_refused() {
    let w = world(&[("SPEEDYSPEC_TEST_NEVER_READY_VERSION", "0.2.0")], FAST);
    let (old_pid, _, _) = w.generation();
    publish_and_install(&w, "0.2.0");
    let (code, _, stderr) = w.cmd(&["release", "switch", "server@0.2.0"]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("0.2.0") && stderr.contains("ready"),
        "{stderr}"
    );
    assert_eq!(w.generation().0, old_pid);
    assert_eq!(get(w.addr, "/").1, "admin-1.0.0");
    assert!(w.state()["server"]["failed"]["0.2.0"].is_string());
}

#[test]
fn binary_exiting_before_ready_is_refused() {
    let w = world(&[("SPEEDYSPEC_TEST_CRASH_VERSION", "0.2.0")], FAST);
    let (old_pid, _, _) = w.generation();
    publish_and_install(&w, "0.2.0");
    assert_eq!(w.cmd(&["release", "switch", "server@0.2.0"]).0, 1);
    assert_eq!(w.generation().0, old_pid);
    assert_eq!(get(w.addr, "/").1, "admin-1.0.0");
}

#[test]
fn second_upgrade_refused_during_trial() {
    let w = world(
        &[],
        "trial_period = \"10s\"\nready_timeout = \"5s\"\ndrain_timeout = \"5s\"",
    );
    publish_and_install(&w, "0.2.0");
    publish_and_install(&w, "0.3.0");
    w.ok(&["release", "switch", "server@0.2.0"]);
    let (code, _, stderr) = w.cmd(&["release", "switch", "server@0.3.0"]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("0.2.0") && stderr.contains("trial"),
        "{stderr}"
    );
}

#[test]
fn group_switch_with_server() {
    let w = world(&[], FAST);
    w.host
        .publish_static_requiring("admin", "2.0.0", r#""server":"^0.2""#);
    w.ok(&["release", "install", "admin", "2.0.0"]);
    publish_and_install(&w, "0.2.0");
    assert_eq!(
        w.cmd(&["release", "switch", "admin@2.0.0"]).0,
        1,
        "admin 2 alone needs server 0.2"
    );
    w.ok(&["release", "switch", "admin@2.0.0", "server@0.2.0"]);
    assert_eq!(
        poll(w.addr, "/", Duration::from_secs(5), |_, b| b
            == "admin-2.0.0")
        .1,
        "admin-2.0.0"
    );
    assert!(wait_for(Duration::from_secs(8), || w.generation().2 == "confirmed"));
    assert_eq!(w.state()["apps"]["admin"]["active"], "2.0.0");
}

#[test]
fn failed_group_trial_reverts_apps() {
    let w = world(
        &[
            ("SPEEDYSPEC_TEST_EXIT_VERSION", "0.2.0"),
            ("SPEEDYSPEC_TEST_EXIT_AFTER_READY_MS", "1500"),
        ],
        FAST,
    );
    w.host
        .publish_static_requiring("admin", "2.0.0", r#""server":"^0.2""#);
    w.ok(&["release", "install", "admin", "2.0.0"]);
    publish_and_install(&w, "0.2.0");
    w.ok(&["release", "switch", "admin@2.0.0", "server@0.2.0"]);
    assert!(wait_for(Duration::from_secs(10), || {
        w.state()["server"]["failed"]["0.2.0"].is_string()
    }));
    assert_eq!(
        poll(w.addr, "/", Duration::from_secs(5), |_, b| b
            == "admin-1.0.0")
        .1,
        "admin-1.0.0"
    );
    assert_eq!(w.state()["apps"]["admin"]["active"], "1.0.0");
}

#[test]
fn server_rollback_is_an_upgrade_to_older_binary() {
    let w = world(&[], FAST);
    publish_and_install(&w, "0.2.0");
    w.ok(&["release", "switch", "server@0.2.0"]);
    assert!(wait_for(Duration::from_secs(8), || w.generation().2 == "confirmed"));
    assert!(
        w.ok(&["release", "rollback", "server", BASE])
            .contains(BASE)
    );
    assert!(wait_for(Duration::from_secs(8), || {
        let (_, version, trial) = w.generation();
        version == BASE && trial == "confirmed"
    }));
}

#[test]
fn sigterm_during_upgrade_exits_cleanly() {
    let mut w = world(
        &[],
        "trial_period = \"20s\"\nready_timeout = \"5s\"\ndrain_timeout = \"3s\"",
    );
    let (old_pid, _, _) = w.generation();
    publish_and_install(&w, "0.2.0");
    w.ok(&["release", "switch", "server@0.2.0"]);
    let (new_pid, _, _) = w.generation();
    w.keeper.term();
    assert!(wait_for(Duration::from_secs(10), || !alive(old_pid)
        && !alive(new_pid)));
}
