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
    world_with(env, handoff, "")
}

fn world_with(env: &[(&str, &str)], handoff: &str, extra_listener: &str) -> World {
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
{extra_listener}
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
        // The trial generation crashes on purpose; a request it had just accepted is reset with it, which clients
        // retry. A failure is one whose retry 100 ms later also fails.
        let once = || std::panic::catch_unwind(|| get(w.addr, "/").0).ok();
        if once() != Some(200) {
            std::thread::sleep(Duration::from_millis(100));
            if once() != Some(200) {
                failures += 1;
            }
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

/// Spec "Testing": handoff under load. TCP requests never fail; HTTP/3 requests fail only with the upgrade close and
/// succeed on an immediate retry; a slow response opened before the first upgrade finishes on the old generation.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn handoff_under_load() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    let certs_dir = tempfile::tempdir().unwrap();
    let certs = support::certs::generate(certs_dir.path());
    let tls = format!(
        "\n[[listener]]\naddress = \"127.0.0.1:0\"\ntls = {{ cert = \"{}\", key = \"{}\" }}\nhttp3 = true\n",
        certs.tls.cert.display(),
        certs.tls.key.display()
    );
    let w = tokio::task::spawn_blocking(move || {
        world_with(
            &[],
            "trial_period = \"1s\"\nready_timeout = \"5s\"\ndrain_timeout = \"10s\"",
            &tls,
        )
    })
    .await
    .unwrap();
    let found = {
        let out = w.keeper.log();
        let mut tcp = Vec::new();
        let mut udp = None;
        for line in out {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line)
                && value["event"] == "listening"
            {
                let addr: std::net::SocketAddr =
                    value["address"].as_str().unwrap().parse().unwrap();
                if value["protocol"] == "tcp" {
                    tcp.push(addr)
                } else {
                    udp = Some(addr)
                }
            }
        }
        (tcp, udp.expect("an h3 listener"))
    };
    let (plain, secure, quic) = (found.0[0], found.0[1], found.1);
    for version in ["0.2.0", "0.3.0"] {
        w.host.publish_server(version, Path::new(BIN));
        w.ok(&["release", "install", "server", version]);
    }
    let (first_pid, _, _) = w.generation();

    let slow = tokio::spawn(async move {
        let client = reqwest::Client::builder().http1_only().build().unwrap();
        client
            .get(format!("http://{plain}/__speedyspec_test/slow?seconds=6"))
            .send()
            .await?
            .text()
            .await
    });
    tokio::time::sleep(Duration::from_millis(300)).await;

    let stop = Arc::new(AtomicBool::new(false));
    let failures: Arc<Mutex<Vec<String>>> = Arc::default();
    let ca = reqwest::Certificate::from_pem(&std::fs::read(&certs.ca).unwrap()).unwrap();
    let clients = [
        (
            "h1",
            format!("http://{plain}/"),
            reqwest::Client::builder().http1_only().build().unwrap(),
        ),
        (
            "h2c",
            format!("http://{plain}/"),
            reqwest::Client::builder()
                .http2_prior_knowledge()
                .build()
                .unwrap(),
        ),
        (
            "h2",
            format!("https://localhost:{}/", secure.port()),
            reqwest::Client::builder()
                .add_root_certificate(ca)
                .build()
                .unwrap(),
        ),
    ];
    let mut tasks = Vec::new();
    for (name, url, client) in clients {
        let (stop, failures) = (stop.clone(), failures.clone());
        tasks.push(tokio::spawn(async move {
            let mut served = 0;
            while !stop.load(Ordering::SeqCst) {
                match client.get(&url).send().await {
                    Ok(response) if response.status() == 200 => served += 1,
                    Ok(response) => failures
                        .lock()
                        .unwrap()
                        .push(format!("{name}: {}", response.status())),
                    Err(err) => failures.lock().unwrap().push(format!("{name}: {err}")),
                }
            }
            served
        }));
    }
    {
        let (stop, failures, ca) = (stop.clone(), failures.clone(), certs.ca.clone());
        tasks.push(tokio::spawn(async move {
            let mut served = 0;
            while !stop.load(Ordering::SeqCst) {
                match support::h3::try_get(quic, &ca, "/").await {
                    Ok((200, _, _)) => served += 1,
                    Ok((status, _, _)) => failures.lock().unwrap().push(format!("h3: {status}")),
                    // The upgrade close is expected; a retry must succeed at once.
                    // A connection still in its handshake can only get a bare transport close (RFC 9000 §10.2.3).
                    // A graceful H3_NO_ERROR termination is a server going away, which clients retry (RFC 9114 §5.2).
                    Err(err)
                        if err.contains("21331")
                            || err.contains("H3_NO_ERROR")
                            || err.contains("0x5353")
                            || err.contains("APPLICATION_ERROR")
                            || err.contains("closed during the handshake") =>
                    {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        if support::h3::try_get(quic, &ca, "/").await.is_err() {
                            failures
                                .lock()
                                .unwrap()
                                .push(format!("h3 retry after upgrade close: {err}"));
                        }
                    }
                    Err(err) => failures.lock().unwrap().push(format!("h3: {err}")),
                }
            }
            served
        }));
    }

    let config = w.config.to_str().unwrap().to_string();
    let switch = |target: &'static str, config: String| {
        tokio::task::spawn_blocking(move || {
            command(&["release", "switch", "--config", &config, target])
        })
    };
    tokio::time::sleep(Duration::from_secs(2)).await;
    let (code, _, stderr) = switch("server@0.2.0", config.clone()).await.unwrap();
    assert_eq!(code, 0, "{stderr}");
    tokio::time::sleep(Duration::from_secs(3)).await;
    // The first generation may still be draining the slow response; the keeper refuses until it is gone.
    let deadline = Instant::now() + Duration::from_secs(15);
    let (code, _, stderr) = loop {
        let result = switch("server@0.3.0", config.clone()).await.unwrap();
        if result.0 == 0 || !result.2.contains("still draining") || Instant::now() > deadline {
            break result;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    assert_eq!(code, 0, "{stderr}");
    tokio::time::sleep(Duration::from_secs(3)).await;
    stop.store(true, Ordering::SeqCst);
    let mut served = Vec::new();
    for task in tasks {
        served.push(task.await.unwrap());
    }

    let failures = failures.lock().unwrap().clone();
    if !failures.is_empty() {
        let mut kinds: std::collections::BTreeMap<String, usize> = Default::default();
        for failure in &failures {
            *kinds.entry(failure.chars().take(90).collect()).or_default() += 1;
        }
        eprintln!("failure kinds: {kinds:#?}");
        for line in w
            .keeper
            .log()
            .iter()
            .filter(|l| l.contains("\"event\":\"") && !l.contains("listening"))
        {
            eprintln!("log: {}", line.chars().take(200).collect::<String>());
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures, first: {:?}",
        failures.len(),
        &failures[..failures.len().min(5)]
    );
    assert!(served.iter().all(|n| *n > 0), "{served:?}");
    let body = slow.await.unwrap().expect("the slow response completes");
    let pids: Vec<&str> = body.lines().collect();
    assert_eq!(pids.len(), 6, "{body:?}");
    assert!(
        pids.iter().all(|pid| *pid == first_pid.to_string()),
        "served by {pids:?}, first generation {first_pid}"
    );
}

/// The `keeper` line of `status`.
fn keeper_version(w: &World) -> String {
    let out = w.ok(&["status"]);
    out.lines()
        .find_map(|l| l.strip_prefix("keeper\t"))
        .unwrap_or_else(|| panic!("{out}"))
        .to_string()
}

#[test]
fn reload_picks_up_new_mount() {
    let w = world(&[], FAST);
    let (old_pid, _, _) = w.generation();
    let toml = std::fs::read_to_string(&w.config).unwrap().replace(
        "path = \"/\"\napp = \"admin\"",
        "path = \"/site/\"\napp = \"admin\"",
    );
    std::fs::write(&w.config, toml).unwrap();
    assert!(
        w.ok(&["reload"]).starts_with("reloaded"),
        "the reload reports"
    );
    assert_eq!(
        poll(w.addr, "/site/", Duration::from_secs(5), |s, _| s == 200).1,
        "admin-1.0.0"
    );
    assert_ne!(w.generation().0, old_pid);
    assert!(wait_for(Duration::from_secs(8), || w.generation().2 == "confirmed"));
    assert!(
        w.state()["server"]["failed"]
            .as_object()
            .is_none_or(|f| f.is_empty())
    );
}

#[test]
fn reload_refuses_listener_change() {
    let w = world(&[], FAST);
    let toml = std::fs::read_to_string(&w.config).unwrap().replace(
        "address = \"127.0.0.1:0\"",
        "address = \"127.0.0.1:0\"\nhttp3 = false\n[[listener]]\naddress = \"127.0.0.1:0\"",
    );
    std::fs::write(&w.config, toml).unwrap();
    let (code, _, stderr) = w.cmd(&["reload"]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("listener changes need a full restart"),
        "{stderr}"
    );
}

#[test]
fn keeper_reexecs_after_upgrade() {
    let w = world(&[], FAST);
    let keeper_pid = w.keeper.child.id();
    assert_eq!(keeper_version(&w), BASE);
    publish_and_install(&w, "0.2.0");
    w.ok(&["release", "switch", "server@0.2.0"]);
    let mut failures = 0;
    let reexecuted = wait_for(Duration::from_secs(15), || {
        if get(w.addr, "/").0 != 200 {
            failures += 1;
        }
        command(&["status", "--config", w.config.to_str().unwrap()])
            .1
            .contains("keeper\t0.2.0")
    });
    assert!(reexecuted, "log: {:?}", w.keeper.log());
    assert_eq!(failures, 0);
    assert!(alive(keeper_pid), "the keeper keeps its PID");
    assert!(
        w.keeper
            .wait_for_event("keeper_reexecuted", Duration::from_secs(2))
    );
    assert_eq!(get(w.addr, "/").1, "admin-1.0.0");
    assert_eq!(w.generation().1, "0.2.0");
}

#[test]
fn newer_protocol_binary_refused() {
    let w = world(&[], FAST);
    publish_and_install(&w, "0.2.0");
    let kept = w
        .dir
        .path()
        .join("data/releases/server/0.2.0/speedyspec-server.test-protocol");
    std::fs::write(kept, "99").unwrap();
    let (code, _, stderr) = w.cmd(&["release", "switch", "server@0.2.0"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("protocol 99"), "{stderr}");
}

#[tokio::test(flavor = "multi_thread")]
async fn keeper_reexec_keeps_listeners_and_socket() {
    let certs_dir = tempfile::tempdir().unwrap();
    let certs = support::certs::generate(certs_dir.path());
    let tls = format!(
        "\n[[listener]]\naddress = \"127.0.0.1:0\"\ntls = {{ cert = \"{}\", key = \"{}\" }}\nhttp3 = true\n",
        certs.tls.cert.display(),
        certs.tls.key.display()
    );
    let w = tokio::task::spawn_blocking(move || {
        let w = world_with(&[], FAST, &tls);
        publish_and_install(&w, "0.2.0");
        w.ok(&["release", "switch", "server@0.2.0"]);
        assert!(
            wait_for(Duration::from_secs(15), || keeper_version(&w) == "0.2.0"),
            "log: {:?}",
            w.keeper.log()
        );
        w
    })
    .await
    .unwrap();
    let (mut secure, mut quic) = (None, None);
    for line in w.keeper.log() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line)
            && value["event"] == "listening"
        {
            let addr: std::net::SocketAddr = value["address"].as_str().unwrap().parse().unwrap();
            if value["protocol"] == "tcp" {
                if addr != w.addr {
                    secure = Some(addr);
                }
            } else {
                quic = Some(addr);
            }
        }
    }
    let (secure, quic) = (
        secure.expect("a TLS listener"),
        quic.expect("an h3 listener"),
    );
    let ca = reqwest::Certificate::from_pem(&std::fs::read(&certs.ca).unwrap()).unwrap();
    let client = reqwest::Client::builder()
        .add_root_certificate(ca)
        .build()
        .unwrap();
    let response = client
        .get(format!("https://localhost:{}/", secure.port()))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let (status, _, _) = support::h3::try_get(quic, &certs.ca, "/").await.unwrap();
    assert_eq!(status, 200);
    let w = tokio::task::spawn_blocking(move || {
        assert_eq!(get(w.addr, "/").0, 200);
        assert!(
            w.ok(&["status"]).contains("generation\t"),
            "keeper.sock answers"
        );
        w
    })
    .await
    .unwrap();
    drop(w);
}

/// Waits for the keeper to exit, up to `timeout`.
fn keeper_exits(w: &mut World, timeout: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(status) = w.keeper.child.try_wait().unwrap() {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

/// Opens `/__speedyspec_test/slow` and keeps reading it on a thread, so the generation serving it stays busy.
fn hold_slow_request(addr: std::net::SocketAddr, seconds: u32) -> std::thread::JoinHandle<()> {
    use std::io::{Read, Write};

    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    write!(
        stream,
        "GET /__speedyspec_test/slow?seconds={seconds} HTTP/1.1\r\nHost: x\r\n\r\n"
    )
    .unwrap();
    let mut first = [0u8; 12];
    stream.read_exact(&mut first).unwrap();
    std::thread::spawn(move || {
        let mut sink = Vec::new();
        let _ = stream.read_to_end(&mut sink);
    })
}

#[test]
fn sigterm_during_crash_restarts_exits() {
    let mut w = world(&[], FAST);
    let (pid, _, _) = w.generation();
    let binary = w.dir.path().join(format!(
        "data/releases/server/{BASE}/speedyspec-server.test-never-ready"
    ));
    std::fs::write(binary, "1").unwrap();
    Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status()
        .unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    Command::new("kill")
        .args(["-TERM", &w.keeper.child.id().to_string()])
        .status()
        .unwrap();
    let status = keeper_exits(&mut w, Duration::from_secs(5));
    assert!(
        status.is_some_and(|s| s.success()),
        "the keeper ignored SIGTERM while restarting: {status:?}"
    );
}

#[test]
fn retiring_generation_blocks_upgrades_and_is_stopped() {
    let mut w = world(
        &[],
        "trial_period = \"2s\"\nready_timeout = \"5s\"\ndrain_timeout = \"20s\"",
    );
    publish_and_install(&w, "0.2.0");
    publish_and_install(&w, "0.3.0");
    let (old_pid, _, _) = w.generation();
    let _slow = hold_slow_request(w.addr, 30);
    w.ok(&["release", "switch", "server@0.2.0"]);
    assert!(wait_for(Duration::from_secs(8), || w.generation().2 == "confirmed"));
    assert!(
        alive(old_pid),
        "the old generation drains its open response"
    );
    let (code, _, stderr) = w.cmd(&["release", "switch", "server@0.3.0"]);
    assert_eq!(
        code, 1,
        "an upgrade during the previous generation's drain is refused"
    );
    assert!(stderr.contains("still draining"), "{stderr}");
    assert_eq!(
        w.state()["server"]["trial"],
        serde_json::Value::Null,
        "the refused switch left no trial"
    );
    Command::new("kill")
        .args(["-TERM", &w.keeper.child.id().to_string()])
        .status()
        .unwrap();
    assert!(keeper_exits(&mut w, Duration::from_secs(30)).is_some());
    assert!(
        !alive(old_pid),
        "the draining generation outlived its keeper"
    );
}

#[test]
fn switch_refused_during_reload_trial_leaves_no_trial() {
    let w = world(&[], FAST);
    publish_and_install(&w, "0.2.0");
    w.ok(&["reload"]);
    let (code, _, stderr) = w.cmd(&["release", "switch", "server@0.2.0"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("a reload is on trial"), "{stderr}");
    let state = w.state();
    assert_eq!(state["server"]["trial"], serde_json::Value::Null, "{state}");
    assert_eq!(state["pending_group"], serde_json::Value::Null, "{state}");
}

#[test]
fn release_commands_refused_while_a_server_switch_starts() {
    let w = world(&[("SPEEDYSPEC_TEST_NEVER_READY_VERSION", "0.2.0")], FAST);
    publish_and_install(&w, "0.2.0");
    let config = w.config.to_str().unwrap().to_string();
    let switch = std::thread::spawn(move || {
        command(&["release", "switch", "server@0.2.0", "--config", &config])
    });
    std::thread::sleep(Duration::from_millis(1500));
    let (code, _, stderr) = w.cmd(&["release", "pin", "admin", "1.0.0"]);
    assert_eq!(
        code, 1,
        "a release command while the new generation starts is refused"
    );
    assert!(stderr.contains("server upgrade is in progress"), "{stderr}");
    assert_eq!(
        switch.join().unwrap().0,
        1,
        "the never-ready binary is refused"
    );
    w.ok(&["release", "pin", "admin", "1.0.0"]);
}

#[test]
fn failed_confirm_fails_the_trial() {
    let w = world(&[("SPEEDYSPEC_TEST_CONFIRM_FAIL_VERSION", "0.2.0")], FAST);
    let (old_pid, _, _) = w.generation();
    publish_and_install(&w, "0.2.0");
    w.ok(&["release", "switch", "server@0.2.0"]);
    assert!(
        wait_for(Duration::from_secs(10), || w.generation().0 == old_pid),
        "the old generation took over again; log: {:?}",
        w.keeper.log()
    );
    let state = w.state();
    assert_eq!(state["server"]["confirmed"], BASE, "{state}");
    assert!(state["server"]["failed"]["0.2.0"].is_string(), "{state}");
    assert_eq!(state["server"]["trial"], serde_json::Value::Null, "{state}");
    assert_eq!(keeper_version(&w), BASE);
}
