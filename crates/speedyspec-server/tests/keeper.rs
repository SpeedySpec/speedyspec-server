//! The keeper process: it owns the sockets and supervises generations.

mod support;

use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use support::run::{BIN, command, get, poll, start, start_with};

fn immutable_config(dir: &Path, extra: &str) -> PathBuf {
    let admin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/admin-a");
    std::fs::create_dir_all(dir.join("run")).unwrap();
    let toml = format!(
        "mode = \"immutable\"\nrun_dir = \"{run}\"\n{extra}\n[[listener]]\naddress = \"127.0.0.1:0\"\n[app.server]\nkind = \"server\"\n[app.admin]\nkind = \"static\"\npath = \"{admin}\"\n[[mount]]\npath = \"/\"\napp = \"admin\"\n",
        run = dir.join("run").display(),
        admin = admin.display(),
    );
    let path = dir.join("server.toml");
    std::fs::write(&path, toml).unwrap();
    path
}

fn status(config: &Path) -> String {
    let (code, stdout, stderr) = command(&["status", "--config", config.to_str().unwrap()]);
    assert_eq!(code, 0, "{stderr}");
    stdout
}

/// The PID on the `generation` line of `status`.
fn generation_pid(config: &Path) -> u32 {
    let out = status(config);
    let line = out
        .lines()
        .find(|l| l.starts_with("generation\t"))
        .unwrap_or_else(|| panic!("{out}"));
    line.split('\t').nth(1).unwrap().parse().unwrap()
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .unwrap()
        .success()
}

fn kill(signal: &str, pid: u32) {
    assert!(
        Command::new("kill")
            .arg(format!("-{signal}"))
            .arg(pid.to_string())
            .status()
            .unwrap()
            .success()
    );
}

fn wait_for(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    done()
}

#[test]
fn keeper_starts_a_generation_and_serves() {
    let dir = tempfile::tempdir().unwrap();
    let config = immutable_config(dir.path(), "");
    let mut keeper = start(&config);
    let addr = keeper.address();
    assert!(
        poll(addr, "/", Duration::from_secs(10), |s, _| s == 200)
            .1
            .contains("admin-a")
    );
    let pid = generation_pid(&config);
    assert_ne!(pid, keeper.child.id());
    assert!(status(&config).contains(&format!("generation\t{pid}\t")));
}

#[test]
fn crashed_generation_is_restarted() {
    let dir = tempfile::tempdir().unwrap();
    let config = immutable_config(dir.path(), "");
    let mut keeper = start(&config);
    let addr = keeper.address();
    poll(addr, "/", Duration::from_secs(10), |s, _| s == 200);
    let first = generation_pid(&config);
    kill("KILL", first);
    assert!(wait_for(Duration::from_secs(5), || get(addr, "/").0 == 200
        && generation_pid(&config) != first));
    assert!(
        keeper.wait_for_event("generation_exited", Duration::from_secs(2)),
        "{:?}",
        keeper.log()
    );
}

#[test]
fn generation_exits_when_keeper_is_killed() {
    let dir = tempfile::tempdir().unwrap();
    let config = immutable_config(dir.path(), "");
    let mut keeper = start(&config);
    let addr = keeper.address();
    poll(addr, "/", Duration::from_secs(10), |s, _| s == 200);
    let generation = generation_pid(&config);
    kill("KILL", keeper.child.id());
    assert!(
        wait_for(Duration::from_secs(3), || !alive(generation)),
        "the generation outlived its keeper"
    );
}

#[test]
fn sigterm_drains_and_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let config = immutable_config(dir.path(), "");
    let mut keeper = start(&config);
    let addr = keeper.address();
    poll(addr, "/", Duration::from_secs(10), |s, _| s == 200);
    let generation = generation_pid(&config);
    kill("TERM", keeper.child.id());
    let started = Instant::now();
    let status = loop {
        if let Some(status) = keeper.child.try_wait().unwrap() {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "keeper did not exit"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(0));
    assert!(!alive(generation));
}

#[test]
fn sighup_reloads_certificates() {
    let dir = tempfile::tempdir().unwrap();
    let certs = support::certs::generate(dir.path());
    let listener = format!(
        "[[listener]]\naddress = \"127.0.0.1:0\"\ntls = {{ cert = \"{}\", key = \"{}\" }}\n",
        certs.tls.cert.display(),
        certs.tls.key.display()
    );
    let config = immutable_config(dir.path(), "");
    let toml = std::fs::read_to_string(&config)
        .unwrap()
        .replace("[app.server]", &format!("{listener}[app.server]"));
    std::fs::write(&config, toml).unwrap();
    let mut keeper = start(&config);
    let found = keeper.listening(2);
    let secure = found.iter().filter(|(p, _)| p == "tcp").nth(1).unwrap().1;
    let client = |ca: &Path| {
        let ca = reqwest::Certificate::from_pem(&std::fs::read(ca).unwrap()).unwrap();
        reqwest::blocking::Client::builder()
            .add_root_certificate(ca)
            .build()
            .unwrap()
    };
    let url = format!("https://localhost:{}/", secure.port());
    assert!(wait_for(Duration::from_secs(10), || client(&certs.ca)
        .get(&url)
        .send()
        .is_ok()));
    let next = support::certs::generate(dir.path());
    kill("HUP", keeper.child.id());
    assert!(wait_for(Duration::from_secs(5), || client(&next.ca)
        .get(&url)
        .send()
        .is_ok()));
}

#[test]
fn notify_ready_sent() {
    let dir = tempfile::tempdir().unwrap();
    let config = immutable_config(dir.path(), "");
    let socket_path = dir.path().join("notify.sock");
    let notify = UnixDatagram::bind(&socket_path).unwrap();
    notify
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut keeper = start_with(&config, &[("NOTIFY_SOCKET", socket_path.to_str().unwrap())]);
    keeper.address();
    let mut buf = [0u8; 64];
    let n = notify.recv(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"READY=1");
}

#[test]
fn stale_keeper_socket_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let config = immutable_config(dir.path(), "");
    drop(std::os::unix::net::UnixListener::bind(dir.path().join("run/keeper.sock")).unwrap());
    let mut keeper = start(&config);
    keeper.address();
    assert!(wait_for(Duration::from_secs(10), || command(&[
        "status",
        "--config",
        config.to_str().unwrap()
    ])
    .0 == 0));
}

#[test]
fn immutable_keeper_restarts_crashes_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let config = immutable_config(dir.path(), "");
    let before: Vec<_> =
        std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/admin-a"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
    let mut keeper = start(&config);
    let addr = keeper.address();
    poll(addr, "/", Duration::from_secs(10), |s, _| s == 200);
    kill("KILL", generation_pid(&config));
    assert!(wait_for(Duration::from_secs(5), || get(addr, "/").0 == 200));
    let after: Vec<_> =
        std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/admin-a"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
    assert_eq!(before.len(), after.len());
}

#[test]
fn managed_first_start_registers_binary() {
    let dir = tempfile::tempdir().unwrap();
    let toml = format!(
        "mode = \"managed\"\ndata_dir = \"{data}\"\nrun_dir = \"{run}\"\n[[listener]]\naddress = \"127.0.0.1:0\"\n[app.server]\nkind = \"server\"\n",
        data = dir.path().join("data").display(),
        run = dir.path().join("run").display(),
    );
    let config = dir.path().join("server.toml");
    std::fs::write(&config, toml).unwrap();
    let mut keeper = start(&config);
    keeper.address();
    let version = env!("CARGO_PKG_VERSION");
    let binary = dir
        .path()
        .join(format!("data/releases/server/{version}/speedyspec-server"));
    assert!(wait_for(Duration::from_secs(10), || binary.exists()));
    let state_file = dir.path().join("data/state.json");
    let confirmed = || {
        std::fs::read_to_string(&state_file)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .is_some_and(|state| state["server"]["confirmed"] == version)
    };
    assert!(
        wait_for(Duration::from_secs(10), confirmed),
        "state.json never confirmed {version}"
    );
    assert!(wait_for(Duration::from_secs(10), || {
        command(&["status", "--config", config.to_str().unwrap()])
            .1
            .contains("generation\t")
    }));
}

#[test]
fn protocol_mismatch_is_refused() {
    use speedyspec_server_handoff::{Channel, GenerationRole, ToGeneration};
    let (keeper, child) = Channel::pair().unwrap();
    let child_fd = std::os::fd::IntoRawFd::into_raw_fd(child);
    let mut command = Command::new(BIN);
    command
        .arg("serve")
        .env("SPEEDYSPEC_CHANNEL_FD", "3")
        .stderr(std::process::Stdio::piped());
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(&mut command, move || {
            if libc::dup2(child_fd, 3) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let process = command.spawn().unwrap();
    unsafe { drop(<std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(child_fd)) };
    keeper
        .send(
            &ToGeneration::Hello {
                protocol: 99,
                config: "/nonexistent".into(),
                sockets: vec![],
                role: GenerationRole::Immutable,
            },
            &[],
        )
        .unwrap();
    let output = process.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("99"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(feature = "test-hooks")]
#[test]
fn crash_loop_backs_off() {
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("speedyspec-server");
    std::fs::copy(BIN, &copy).unwrap();
    std::fs::write(
        dir.path()
            .join("speedyspec-server.test-exit-after-ready-ms"),
        "0",
    )
    .unwrap();
    let config = immutable_config(dir.path(), "");
    let keeper = support::run::start_binary(&copy, &config, &[]);
    std::thread::sleep(Duration::from_secs(4));
    let starts = keeper
        .log()
        .iter()
        .filter(|l| l.contains("\"event\":\"generation_started\""))
        .count();
    assert!(
        (2..=4).contains(&starts),
        "{starts} starts in 4 s: {:?}",
        keeper.log()
    );
}
