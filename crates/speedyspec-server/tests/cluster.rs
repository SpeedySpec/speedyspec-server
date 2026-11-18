//! Several pods on one data directory, each its own keeper, with the one-host lease file (`leadership = "file"`).
#![cfg(feature = "test-hooks")]

mod support;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use support::release_host::ReleaseHost;
use support::run::{BIN, Running, command, get, start_with};

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

/// Pods sharing one data directory and one release host, with the admin static app and the server app.
struct Pods {
    dir: tempfile::TempDir,
    host: ReleaseHost,
    pods: Vec<Pod>,
}

struct Pod {
    name: String,
    keeper: Running,
    config: PathBuf,
    addr: SocketAddr,
}

impl Pods {
    fn new() -> Pods {
        let host = ReleaseHost::start();
        host.publish_static("admin", "1.0.0");
        Pods {
            dir: tempfile::tempdir().unwrap(),
            host,
            pods: Vec::new(),
        }
    }

    fn start(&mut self, name: &str, env: &[(&str, &str)]) {
        self.start_extra(name, env, "");
    }

    /// Starts a pod with `extra` TOML appended to its config (after every table).
    fn start_extra(&mut self, name: &str, env: &[(&str, &str)], extra: &str) {
        let apps = format!(
            r#"
[handoff]
trial_period = "2s"
ready_timeout = "5s"
drain_timeout = "2s"

[app.admin]
kind = "static"
source = "{admin}"
keys = ["{key}"]
update = "manual"

[[mount]]
path = "/"
app = "admin"
"#,
            admin = self.host.url("/admin/index.json"),
            key = self.host.public_key(),
        );
        let config = pod_config(self.dir.path(), name, "");
        let toml = std::fs::read_to_string(&config).unwrap().replace(
            "[app.server]\nkind = \"server\"\n",
            &format!(
                "[app.server]\nkind = \"server\"\nsource = \"{}\"\nkeys = [\"{}\"]\n",
                self.host.url("/server/index.json"),
                self.host.public_key()
            ),
        ) + &apps
            + extra;
        let toml = toml.replacen("[handoff]", "[handoff]", 1);
        // [cluster] is in pod_config's text; the tables appended after it are separate tables.
        std::fs::write(&config, toml).unwrap();
        let mut all = vec![("SPEEDYSPEC_TEST_VERSION_FROM_DIR", "1")];
        all.extend_from_slice(env);
        let mut keeper = start_with(&config, &all);
        let addr = keeper.address();
        let socket = self.dir.path().join(format!("run-{name}/keeper.sock"));
        assert!(
            wait_for(Duration::from_secs(10), || socket.exists()),
            "no keeper.sock for {name}"
        );
        self.pods.push(Pod {
            name: name.to_string(),
            keeper,
            config,
            addr,
        });
    }

    fn pod(&self, name: &str) -> &Pod {
        self.pods.iter().find(|p| p.name == name).unwrap()
    }

    fn cmd(&self, name: &str, args: &[&str]) -> (i32, String, String) {
        let config = self.pod(name).config.to_str().unwrap().to_string();
        let mut all: Vec<&str> = args.to_vec();
        all.extend(["--config", &config]);
        command(&all)
    }

    fn ok(&self, name: &str, args: &[&str]) -> String {
        let (code, out, err) = self.cmd(name, args);
        assert_eq!(code, 0, "{args:?} on {name}: {err}\n{}", self.events());
        out
    }

    /// Every pod's cluster events, for failure messages.
    fn events(&self) -> String {
        let keep = [
            "leader_",
            "took_over",
            "staged",
            "prepared",
            "activated",
            "switch_abandoned",
            "member_",
            "rollout",
            "trial",
            "upgrade",
            "lease",
            "not_leader",
            "follow_failed",
            "stale_epoch",
            "installed",
            "partials",
            "generation_",
        ];
        let mut out = String::new();
        for pod in &self.pods {
            out.push_str(&format!(
                "{} status:\n{}\n",
                pod.name,
                self.cmd(&pod.name, &["status"]).1
            ));
        }
        if let Ok(entries) = std::fs::read_dir(self.dir.path().join("data/cluster/members")) {
            for entry in entries.flatten() {
                out.push_str(&format!(
                    "{}: {}\n",
                    entry.path().display(),
                    std::fs::read_to_string(entry.path()).unwrap_or_default()
                ));
            }
        }
        out.push_str(&format!("state: {}\n", self.state()));
        for pod in &self.pods {
            for line in pod.keeper.log() {
                if keep
                    .iter()
                    .any(|k| line.contains(&format!("\"event\":\"{k}")))
                {
                    out.push_str(&format!(
                        "{}: {}\n",
                        pod.name,
                        line.chars().take(260).collect::<String>()
                    ));
                }
            }
        }
        out
    }

    /// The leader as `name` sees it.
    fn leader(&self, name: &str) -> Option<String> {
        let out = self.cmd(name, &["status"]).1;
        out.lines()
            .find_map(|l| l.strip_prefix("cluster\t"))
            .and_then(|rest| rest.split('\t').nth(2))
            .filter(|l| *l != "-")
            .map(str::to_string)
    }

    /// (pid, version, trial|confirmed) of `name`'s active generation.
    fn generation(&self, name: &str) -> Option<(u32, String, String)> {
        let out = self.cmd(name, &["status"]).1;
        let line = out.lines().find_map(|l| l.strip_prefix("generation\t"))?;
        let mut parts = line.split('\t');
        Some((
            parts.next()?.parse().ok()?,
            parts.next()?.to_string(),
            parts.next()?.to_string(),
        ))
    }

    /// `state.json`, or null before the leader has written it.
    fn state(&self) -> serde_json::Value {
        std::fs::read_to_string(self.dir.path().join("data/state.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(serde_json::Value::Null)
    }

    /// Installs and switches admin 1.0.0 on the leader, and waits until every pod serves it.
    fn serve_admin(&self, leader: &str) {
        self.ok(leader, &["release", "install", "admin", "1.0.0"]);
        self.ok(leader, &["release", "switch", "admin@1.0.0"]);
        for pod in &self.pods {
            assert!(
                wait_for(Duration::from_secs(15), || get(pod.addr, "/").1
                    == "admin-1.0.0"),
                "{} never served admin 1.0.0\n{}",
                pod.name,
                self.events()
            );
        }
    }
}

/// Starts `first` alone until it leads, then the others: the leader is `first`.
fn cluster(
    first: &str,
    others: &[&str],
    env: impl Fn(&str) -> Vec<(&'static str, &'static str)>,
) -> Pods {
    let mut pods = Pods::new();
    pods.start(first, &env(first));
    assert!(
        wait_for(Duration::from_secs(10), || pods.leader(first).as_deref()
            == Some(first)),
        "{first} never led"
    );
    for name in others {
        pods.start(name, &env(name));
    }
    for pod in &pods.pods {
        assert!(
            wait_for(Duration::from_secs(10), || pods
                .leader(&pod.name)
                .as_deref()
                == Some(first)),
            "{} does not see {first} as leader",
            pod.name
        );
    }
    pods
}

/// Watches every pod while `body` runs: (requests that failed, the most pods on trial at once, the order pods
/// reached `version` confirmed).
fn watching(pods: &Pods, version: &str, body: impl FnOnce()) -> (usize, usize, Vec<String>) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    let stop = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(Mutex::new((0usize, 0usize, Vec::<String>::new())));
    let targets: Vec<(String, SocketAddr, PathBuf)> = pods
        .pods
        .iter()
        .map(|p| (p.name.clone(), p.addr, p.config.clone()))
        .collect();
    let monitor = {
        let (stop, seen, version) = (stop.clone(), seen.clone(), version.to_string());
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let mut on_trial = 0;
                for (name, addr, config) in &targets {
                    let answer = std::panic::catch_unwind(|| get(*addr, "/"));
                    if answer.ok().map(|(status, _)| status) != Some(200) {
                        seen.lock().unwrap().0 += 1;
                    }
                    let out = command(&["status", "--config", config.to_str().unwrap()]).1;
                    if let Some(line) = out.lines().find(|l| l.starts_with("generation\t")) {
                        if line.ends_with("\ttrial") {
                            on_trial += 1;
                        }
                        if line.contains(&format!("\t{version}\tconfirmed")) {
                            let mut seen = seen.lock().unwrap();
                            if !seen.2.contains(name) {
                                seen.2.push(name.clone());
                            }
                        }
                    }
                }
                let mut seen = seen.lock().unwrap();
                seen.1 = seen.1.max(on_trial);
            }
        })
    };
    body();
    stop.store(true, Ordering::SeqCst);
    monitor.join().unwrap();
    let seen = seen.lock().unwrap();
    (seen.0, seen.1, seen.2.clone())
}

#[test]
fn rolling_upgrade_goes_pod_by_pod() {
    let pods = cluster("c", &["a", "b"], |_| Vec::new());
    pods.serve_admin("c");
    pods.host.publish_server("0.2.0", Path::new(BIN));
    pods.ok("c", &["release", "install", "server", "0.2.0"]);
    let (failed, most_on_trial, order) = watching(&pods, "0.2.0", || {
        let out = pods.ok("c", &["release", "switch", "server@0.2.0"]);
        assert!(
            out.contains("rolling out server 0.2.0 across 3 pods"),
            "{out}"
        );
        assert!(
            wait_for(Duration::from_secs(90), || {
                let state = pods.state();
                state["server"]["confirmed"] == "0.2.0" && state["rollout"].is_null()
            }),
            "the rollout never finished: {}",
            pods.state()
        );
    });
    assert_eq!(failed, 0, "requests failed during the rollout");
    assert!(
        most_on_trial <= 1,
        "{most_on_trial} pods were on trial at once"
    );
    assert_eq!(order, vec!["a", "b", "c"], "pods by name, the leader last");
    for pod in &pods.pods {
        assert_eq!(pods.generation(&pod.name).unwrap().1, "0.2.0");
    }
}

#[test]
fn rolling_upgrade_stops_and_reverts_on_one_failed_trial() {
    let pods = cluster("c", &["a", "b"], |name| {
        if name == "b" {
            vec![
                ("SPEEDYSPEC_TEST_EXIT_VERSION", "0.2.0"),
                ("SPEEDYSPEC_TEST_EXIT_AFTER_READY_MS", "1000"),
            ]
        } else {
            Vec::new()
        }
    });
    pods.serve_admin("c");
    pods.host.publish_server("0.2.0", Path::new(BIN));
    pods.ok("c", &["release", "install", "server", "0.2.0"]);
    pods.ok("c", &["release", "switch", "server@0.2.0"]);
    assert!(
        wait_for(Duration::from_secs(90), || {
            let state = pods.state();
            state["rollout"].is_null() && state["server"]["failed"]["0.2.0"].is_string()
        }),
        "the rollout never reverted: {}",
        pods.state()
    );
    let reason = pods.state()["server"]["failed"]["0.2.0"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(reason.starts_with("b: "), "{reason}");
    for name in ["a", "b", "c"] {
        assert!(
            wait_for(Duration::from_secs(20), || pods
                .generation(name)
                .is_some_and(|g| g.1 == "0.1.0" && g.2 == "confirmed")),
            "{name} is not back on 0.1.0: {:?}",
            pods.generation(name)
        );
    }
    assert!(
        !pods
            .pod("c")
            .keeper
            .log()
            .iter()
            .any(|l| l.contains("\"generation_started\"") && l.contains("server/0.2.0")),
        "c never upgraded"
    );
}

#[test]
fn follower_first_start_does_not_write_shared_store() {
    let mut pods = Pods::new();
    pods.start("a", &[]);
    let kept = pods.dir.path().join("data/releases/server");
    let listing = || -> Vec<String> {
        let mut names: Vec<String> = walk(&kept);
        names.sort();
        names
    };
    assert!(
        wait_for(Duration::from_secs(10), || pods.leader("a").as_deref()
            == Some("a")
            && pods.state()["server"]["confirmed"]
                == env!("CARGO_PKG_VERSION")),
        "the leader keeps its running binary as the confirmed release"
    );
    assert!(
        wait_for(Duration::from_secs(10), || pods
            .pod("a")
            .keeper
            .log()
            .iter()
            .any(|l| l.contains("\"event\":\"took_over\""))),
        "the leader finishes taking over before the snapshot"
    );
    let (store, state) = (
        listing(),
        std::fs::read(pods.dir.path().join("data/state.json")).unwrap(),
    );
    pods.start("b", &[]);
    assert!(wait_for(Duration::from_secs(10), || pods
        .leader("b")
        .as_deref()
        == Some("a")));
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        listing(),
        store,
        "the follower wrote nothing to the release store"
    );
    assert_eq!(
        std::fs::read(pods.dir.path().join("data/state.json")).unwrap(),
        state,
        "nor to state.json"
    );
    assert_eq!(pods.generation("b").unwrap().1, env!("CARGO_PKG_VERSION"));
}

/// Every file under `dir`, relative.
fn walk(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else {
                out.push(path.display().to_string());
            }
        }
    }
    out
}

/// The pod with this name's keeper and its generation's PID.
fn generation_pid(pods: &Pods, name: &str) -> u32 {
    pods.generation(name).expect("a generation").0
}

fn signal(name: &str, pid: u32) {
    std::process::Command::new("kill")
        .args([&format!("-{name}"), &pid.to_string()])
        .status()
        .unwrap();
}

#[test]
fn two_step_switch_lands_on_all_three_within_one_poll() {
    let pods = cluster("c", &["a", "b"], |_| Vec::new());
    pods.ok("c", &["release", "install", "admin", "1.0.0"]);
    pods.ok("c", &["release", "switch", "admin@1.0.0"]);
    let switched = Instant::now();
    for pod in &pods.pods {
        assert!(
            wait_for(Duration::from_secs(2), || get(pod.addr, "/").1
                == "admin-1.0.0"),
            "{} did not serve the new set: {}",
            pod.name,
            pods.events()
        );
    }
    // poll_interval is 300 ms; allow scheduling slack on top of one poll.
    assert!(
        switched.elapsed() < Duration::from_millis(1500),
        "{:?}",
        switched.elapsed()
    );
}

#[test]
fn member_failing_prepare_goes_not_ready_others_switch() {
    let mut pods = Pods::new();
    pods.start("c", &[]);
    assert!(wait_for(Duration::from_secs(10), || pods
        .leader("c")
        .as_deref()
        == Some("c")));
    pods.start("a", &[]);
    pods.start_extra("b", &[], "\n[limits]\nstatic_memory = 10\n");
    for name in ["a", "b"] {
        assert!(wait_for(Duration::from_secs(10), || pods
            .leader(name)
            .as_deref()
            == Some("c")));
    }
    let probe_b = probe_address(&pods.pod("b").keeper);
    assert!(wait_for(Duration::from_secs(10), || get(
        probe_b, "/readyz"
    )
    .0 == 200));
    pods.ok("c", &["release", "install", "admin", "1.0.0"]);
    pods.ok("c", &["release", "switch", "admin@1.0.0"]);
    for name in ["a", "c"] {
        let addr = pods.pod(name).addr;
        assert!(
            wait_for(Duration::from_secs(5), || get(addr, "/").1 == "admin-1.0.0"),
            "{name}: {}",
            pods.events()
        );
    }
    assert!(
        wait_for(Duration::from_secs(5), || get(probe_b, "/readyz").0 == 503),
        "b cannot hold the release in memory, so it reports not ready: {:?}",
        get(probe_b, "/readyz")
    );
    assert_ne!(
        get(pods.pod("b").addr, "/").1,
        "admin-1.0.0",
        "b keeps its old set"
    );
    assert!(
        pods.cmd("c", &["status"]).1.contains("member\tb\tlive\t"),
        "b is still a live member"
    );
}

#[test]
fn killing_leader_mid_switch_hands_over() {
    let pods = cluster("c", &["a", "b"], |_| Vec::new());
    pods.ok("c", &["release", "install", "admin", "1.0.0"]);
    // b stops answering, so c waits for it (up to the staleness window) with the set staged.
    let b = generation_pid(&pods, "b");
    signal("STOP", b);
    let config = pods.pod("c").config.to_str().unwrap().to_string();
    let switching = std::thread::spawn(move || {
        command(&["release", "switch", "admin@1.0.0", "--config", &config])
    });
    assert!(
        wait_for(Duration::from_secs(5), || pods.state()["staged"]
            .is_object()),
        "nothing was staged: {}",
        pods.state()
    );
    signal("KILL", pods.pod("c").keeper.child.id());
    let killed = Instant::now();
    signal("CONT", b);
    let _ = switching.join();
    assert!(
        wait_for(Duration::from_secs(15), || matches!(
            pods.leader("a").as_deref(),
            Some("a") | Some("b")
        )),
        "no new leader: {}",
        pods.events()
    );
    assert!(killed.elapsed() < Duration::from_secs(15));
    assert!(
        wait_for(Duration::from_secs(20), || {
            let state = pods.state();
            state["staged"].is_null() && state["apps"]["admin"]["active"] == "1.0.0"
        }),
        "the new leader did not finish the switch: {}\n{}",
        pods.state(),
        pods.events()
    );
    for name in ["a", "b"] {
        let addr = pods.pod(name).addr;
        assert!(
            wait_for(Duration::from_secs(10), || get(addr, "/").1
                == "admin-1.0.0"),
            "{name}"
        );
    }
}

#[test]
fn deposed_leader_late_write_ignored() {
    let pods = cluster("c", &["a", "b"], |_| Vec::new());
    pods.serve_admin("c");
    let first_epoch = pods.state()["epoch"].as_u64().unwrap();
    let c = generation_pid(&pods, "c");
    signal("STOP", c);
    let stopped = Instant::now();
    let new_leader = || {
        ["a", "b"]
            .into_iter()
            .find(|n| pods.leader(n).as_deref() == Some(*n))
    };
    assert!(
        wait_for(Duration::from_secs(15), || new_leader().is_some()),
        "no takeover: {}",
        pods.events()
    );
    assert!(
        stopped.elapsed() >= Duration::from_secs(2),
        "not before c's renew deadline"
    );
    let leader = new_leader().unwrap();
    pods.ok(leader, &["release", "pin", "admin", "1.0.0"]);
    let after = pods.state();
    assert!(after["epoch"].as_u64().unwrap() > first_epoch);
    signal("CONT", c);
    assert!(
        wait_for(Duration::from_secs(10), || pods.leader("c").as_deref()
            == Some(leader)),
        "c sees it is no longer the leader: {}",
        pods.cmd("c", &["status"]).1
    );
    let (code, _, stderr) = pods.cmd("c", &["release", "unpin", "admin"]);
    assert_eq!(code, 1, "the deposed leader's writes are refused");
    assert!(
        stderr.contains(&format!("run release commands on {leader}")),
        "{stderr}"
    );
    std::thread::sleep(Duration::from_secs(2));
    let now = pods.state();
    assert_eq!(
        now["epoch"], after["epoch"],
        "nothing c wrote replaced the new leader's state"
    );
    assert_eq!(now["apps"]["admin"]["pinned"], "1.0.0");
}

#[test]
fn follower_refuses_release_commands_with_leader_name() {
    let pods = cluster("c", &["a"], |_| Vec::new());
    let (code, _, stderr) = pods.cmd("a", &["release", "install", "admin", "1.0.0"]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("this pod is not the leader; run release commands on c"),
        "{stderr}"
    );
    let (code, out, _) = pods.cmd("a", &["release", "list"]);
    assert_eq!(code, 0, "reads work on any pod");
    assert!(out.contains("admin\t"), "{out}");
}
