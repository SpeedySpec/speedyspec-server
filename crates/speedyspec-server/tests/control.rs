//! The control socket and the `release` / `status` commands, against the real binary.

mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use support::release_host::ReleaseHost;
use support::run::{Running, command, get, poll, start};

const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Managed {
    dir: tempfile::TempDir,
    host: ReleaseHost,
    config: PathBuf,
    _server: Running,
    addr: std::net::SocketAddr,
}

impl Managed {
    fn cmd(&self, args: &[&str]) -> (i32, String, String) {
        let mut all = vec![];
        all.extend_from_slice(&args[..args.len().min(2)]);
        all.push("--config");
        all.push(self.config.to_str().unwrap());
        all.extend_from_slice(&args[args.len().min(2)..]);
        command(&all)
    }

    fn ok(&self, args: &[&str]) -> String {
        let (code, stdout, stderr) = self.cmd(args);
        assert_eq!(code, 0, "{args:?}: {stderr}");
        stdout
    }
}

fn managed(admin_extra: &str, publish: &[&str]) -> Managed {
    let dir = tempfile::tempdir().unwrap();
    let host = ReleaseHost::start();
    for version in publish {
        host.publish_static("admin", version);
    }
    let toml = format!(
        r#"mode = "managed"
data_dir = "{data}"
run_dir = "{run}"

[[listener]]
address = "127.0.0.1:0"

[app.server]
kind = "server"

[app.admin]
kind = "static"
source = "{source}"
keys = ["{key}"]
{admin_extra}

[[mount]]
path = "/"
app = "admin"
"#,
        data = dir.path().join("data").display(),
        run = dir.path().join("run").display(),
        source = host.url("/admin/index.json"),
        key = host.public_key(),
    );
    let config = dir.path().join("server.toml");
    std::fs::write(&config, toml).unwrap();
    let mut server = start(&config);
    let addr = server.address();
    wait_for_socket(&dir.path().join("run/control.sock"));
    Managed {
        dir,
        host,
        config,
        _server: server,
        addr,
    }
}

fn wait_for_socket(path: &Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "no control socket at {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn install_then_switch_then_rollback() {
    let m = managed("update = \"manual\"", &["1.0.0", "1.1.0"]);
    assert_eq!(
        m.ok(&["release", "install", "admin", "1.0.0"]),
        "installed admin 1.0.0\n"
    );
    assert_eq!(get(m.addr, "/").0, 503);
    assert_eq!(
        m.ok(&["release", "switch", "admin@1.0.0"]),
        "switched admin 1.0.0\n"
    );
    assert_eq!(get(m.addr, "/").1, "admin-1.0.0");
    assert_eq!(
        m.ok(&["release", "install", "admin"]),
        "installed admin 1.1.0\n"
    );
    m.ok(&["release", "switch", "admin@1.1.0"]);
    assert_eq!(get(m.addr, "/").1, "admin-1.1.0");
    assert_eq!(
        m.ok(&["release", "rollback", "admin"]),
        "rolled back admin to 1.0.0\n"
    );
    assert_eq!(get(m.addr, "/").1, "admin-1.0.0");
}

#[test]
fn list_check_and_status_output() {
    let m = managed("update = \"manual\"", &["1.0.0", "1.1.0"]);
    m.ok(&["release", "install", "admin", "1.0.0"]);
    m.ok(&["release", "switch", "admin@1.0.0"]);
    assert_eq!(
        m.ok(&["release", "list"]),
        "admin\tstatic\t1.0.0\t1.0.0\t-\t-\n"
    );
    assert_eq!(
        m.ok(&["release", "list", "admin"]),
        "admin\tstatic\t1.0.0\t1.0.0\t-\t-\n"
    );
    assert_eq!(m.ok(&["release", "check", "admin"]), "admin\t1.1.0\n");
    assert_eq!(
        m.ok(&["status"]),
        format!("server\t{VERSION}\tmanaged\nadmin\tstatic\t1.0.0\t1.0.0\t-\t-\n")
    );
}

#[test]
fn switch_incompatible_refused_with_message() {
    let m = managed("update = \"manual\"", &[]);
    m.host
        .publish_static_requiring("admin", "2.0.0", r#""server":"^99""#);
    m.ok(&["release", "install", "admin"]);
    let (code, _, stderr) = m.cmd(&["release", "switch", "admin@2.0.0"]);
    assert_eq!(code, 1);
    assert_eq!(
        stderr,
        format!("speedyspec-server: admin 2.0.0 requires server ^99, active server is {VERSION}\n")
    );
}

#[test]
fn malformed_switch_target_is_a_usage_error() {
    let m = managed("update = \"manual\"", &[]);
    assert_eq!(m.cmd(&["release", "switch", "admin"]).0, 2);
    assert_eq!(m.cmd(&["release", "switch", "admin@notaversion"]).0, 2);
}

#[test]
fn pin_stops_auto_update() {
    let m = managed("check_interval = \"1s\"", &["1.0.0"]);
    poll(m.addr, "/", Duration::from_secs(10), |s, _| s == 200);
    assert_eq!(
        m.ok(&["release", "pin", "admin", "1.0.0"]),
        "pinned admin 1.0.0\n"
    );
    m.host.publish_static("admin", "1.1.0");
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(get(m.addr, "/").1, "admin-1.0.0");
    assert_eq!(m.ok(&["release", "unpin", "admin"]), "unpinned admin\n");
    let (_, body) = poll(m.addr, "/", Duration::from_secs(10), |_, body| {
        body == "admin-1.1.0"
    });
    assert_eq!(body, "admin-1.1.0");
}

#[test]
fn socket_is_owner_only() {
    let m = managed("update = \"manual\"", &[]);
    let mode = std::fs::metadata(m.dir.path().join("run/control.sock"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn unreachable_socket_message() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("server.toml");
    std::fs::write(
        &config,
        format!(
            "mode = \"immutable\"\nrun_dir = \"{}\"\n[[listener]]\naddress = \"127.0.0.1:0\"\n[app.server]\nkind = \"server\"\n",
            dir.path().join("run").display()
        ),
    )
    .unwrap();
    let (code, _, stderr) = command(&["status", "--config", config.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(
        stderr.starts_with("speedyspec-server: cannot reach the server at "),
        "{stderr}"
    );
    assert!(stderr.contains("is it running?"), "{stderr}");
}

#[test]
fn concurrent_switches_serialize() {
    let m = managed("update = \"manual\"", &["1.0.0", "1.1.0"]);
    m.ok(&["release", "install", "admin", "1.0.0"]);
    m.ok(&["release", "install", "admin", "1.1.0"]);
    let config = m.config.to_str().unwrap().to_string();
    let runs: Vec<_> = ["admin@1.0.0", "admin@1.1.0"]
        .into_iter()
        .map(|target| {
            let config = config.clone();
            std::thread::spawn(move || command(&["release", "switch", "--config", &config, target]))
        })
        .collect();
    for run in runs {
        let (code, _, stderr) = run.join().unwrap();
        assert_eq!(code, 0, "{stderr}");
    }
    let state: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(m.dir.path().join("data/state.json")).unwrap(),
    )
    .unwrap();
    let admin = &state["apps"]["admin"];
    let pair = (
        admin["active"].as_str().unwrap(),
        admin["previous"].as_str().unwrap(),
    );
    assert!(
        pair == ("1.0.0", "1.1.0") || pair == ("1.1.0", "1.0.0"),
        "{pair:?}"
    );
    assert_eq!(get(m.addr, "/").1, format!("admin-{}", pair.0));
}

fn immutable(run: &Path) -> (tempfile::TempDir, PathBuf, Running) {
    let dir = tempfile::tempdir().unwrap();
    let admin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/admin-a");
    let config = dir.path().join("server.toml");
    std::fs::write(
        &config,
        format!(
            "mode = \"immutable\"\nrun_dir = \"{}\"\n[[listener]]\naddress = \"127.0.0.1:0\"\n[app.server]\nkind = \"server\"\n[app.admin]\nkind = \"static\"\npath = \"{}\"\n[[mount]]\npath = \"/\"\napp = \"admin\"\n",
            run.display(),
            admin.display()
        ),
    )
    .unwrap();
    let mut server = start(&config);
    server.address();
    (dir, config, server)
}

#[test]
fn immutable_list_reports_baked_versions() {
    let run = tempfile::tempdir().unwrap();
    let (_dir, config, _server) = immutable(run.path());
    wait_for_socket(&run.path().join("control.sock"));
    let (code, stdout, stderr) =
        command(&["release", "list", "--config", config.to_str().unwrap()]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "admin\tstatic\t1.0.0\t1.0.0\t-\t-\n");
    let (_, status, _) = command(&["status", "--config", config.to_str().unwrap()]);
    assert!(
        status.starts_with(&format!("server\t{VERSION}\timmutable\n")),
        "{status}"
    );
}

#[test]
fn immutable_refuses_changes() {
    let run = tempfile::tempdir().unwrap();
    let (_dir, config, _server) = immutable(run.path());
    wait_for_socket(&run.path().join("control.sock"));
    for args in [
        vec!["release", "install", "admin"],
        vec!["release", "switch", "admin@1.0.0"],
        vec!["release", "rollback", "admin"],
        vec!["release", "pin", "admin", "1.0.0"],
        vec!["release", "unpin", "admin"],
    ] {
        let mut all = args.clone();
        all.insert(2, "--config");
        all.insert(3, config.to_str().unwrap());
        let (code, _, stderr) = command(&all);
        assert_eq!(code, 1, "{args:?}");
        assert!(stderr.contains("speedyspec CLI"), "{args:?}: {stderr}");
    }
}

#[test]
fn second_instance_refuses_and_leaves_the_first_working() {
    let m = managed("update = \"manual\"", &["1.0.0"]);
    m.ok(&["release", "install", "admin", "1.0.0"]);
    let partial = m.dir.path().join("data/releases/admin/9.9.9.partial");
    std::fs::create_dir_all(&partial).unwrap();
    let mut second = std::process::Command::new(support::run::BIN)
        .args(["run", "--config", m.config.to_str().unwrap()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = second.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() > deadline {
            let _ = second.kill();
            let _ = second.wait();
            panic!("the second instance kept running");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(1));
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut second.stderr.take().unwrap(), &mut stderr).unwrap();
    assert!(stderr.contains("already running"), "{stderr}");
    assert!(
        partial.exists(),
        "the second instance must not touch the first one's data"
    );
    assert_eq!(
        m.ok(&["release", "list"]),
        "admin\tstatic\t-\t1.0.0\t-\t-\n",
        "the first server still answers"
    );
}
