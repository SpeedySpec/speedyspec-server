//! End-to-end: the real binary, a temp config, real sockets.

mod support;

use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_speedyspec-server");

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A running server and what it has logged so far.
struct Running {
    child: Child,
    lines: mpsc::Receiver<String>,
    log: Vec<String>,
}

impl Running {
    /// Waits for `count` `listening` events and returns their addresses as `(protocol, address)`.
    fn listening(&mut self, count: usize) -> Vec<(String, SocketAddr)> {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut found = Vec::new();
        while found.len() < count {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self
                .lines
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("no listening lines; log: {:?}", self.log));
            self.log.push(line.clone());
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line)
                && value["event"] == "listening"
            {
                let address = value["address"].as_str().unwrap().parse().unwrap();
                found.push((value["protocol"].as_str().unwrap().to_string(), address));
            }
        }
        found
    }

    fn term(&mut self) -> std::process::ExitStatus {
        let status = Command::new("kill")
            .arg("-TERM")
            .arg(self.child.id().to_string())
            .status()
            .unwrap();
        assert!(status.success());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "did not exit within 5 s of SIGTERM"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(config: &Path, cwd: &Path) -> Running {
    let mut child = Command::new(BIN)
        .arg("run")
        .arg("--config")
        .arg(config)
        .current_dir(cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    Running {
        child,
        lines,
        log: Vec::new(),
    }
}

/// Runs the binary to completion and returns (exit code, stderr).
fn run_to_end(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(BIN).args(args).output().unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

struct Setup {
    dir: tempfile::TempDir,
    config: PathBuf,
}

/// Writes a config with the admin fixture (optionally a copy at `admin_path`) mounted at `/` and the blog at `/blog/`.
fn setup(
    admin_access: &str,
    access_impl: Option<&str>,
    extra_listeners: &str,
    admin_path: Option<&Path>,
) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let admin = admin_path.map_or_else(|| fixture("admin-a"), Path::to_path_buf);
    let access = access_impl
        .map(|a| format!("[access]\nimplementation = \"{a}\"\n"))
        .unwrap_or_default();
    let toml = format!(
        r#"mode = "immutable"

[[listener]]
address = "127.0.0.1:0"
{extra_listeners}
[app.server]
kind = "server"

[app.admin]
kind = "static"
path = "{admin}"

[app.blog]
kind = "php"
path = "{blog}"

[app.rt]
kind = "runtime"
path = "{runtime}"

[[mount]]
path = "/blog/"
app = "blog"

[[mount]]
path = "/"
app = "admin"
access = "{admin_access}"

{access}"#,
        admin = admin.display(),
        blog = fixture("blog").display(),
        runtime = support::runtime::release(
            &dir.path().join("rt"),
            "1.0.0",
            support::runtime::Workers::Real
        )
        .display(),
    );
    let config = dir.path().join("server.toml");
    std::fs::write(&config, toml).unwrap();
    Setup { dir, config }
}

fn first(found: &[(String, SocketAddr)], protocol: &str, index: usize) -> SocketAddr {
    found
        .iter()
        .filter(|(p, _)| p == protocol)
        .nth(index)
        .unwrap()
        .1
}

#[tokio::test]
async fn serves_static_over_h1_h2_h3() {
    let certs_dir = tempfile::tempdir().unwrap();
    let certs = support::certs::generate(certs_dir.path());
    let tls_listener = format!(
        "\n[[listener]]\naddress = \"127.0.0.1:0\"\ntls = {{ cert = \"{}\", key = \"{}\" }}\nhttp3 = true\n",
        certs.tls.cert.display(),
        certs.tls.key.display()
    );
    let setup = setup("public", Some("dev-allow-all"), &tls_listener, None);
    let mut server = start(&setup.config, setup.dir.path());
    let found = server.listening(3);
    let plain = first(&found, "tcp", 0);
    let secure = first(&found, "tcp", 1);
    let quic = first(&found, "udp", 0);
    assert_eq!(secure.port(), quic.port());

    let h1 = reqwest::Client::builder().http1_only().build().unwrap();
    let text = h1
        .get(format!("http://{plain}/"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(text.contains("admin-a"), "{text}");

    let h2c = reqwest::Client::builder()
        .http2_prior_knowledge()
        .build()
        .unwrap();
    let response = h2c.get(format!("http://{plain}/")).send().await.unwrap();
    assert_eq!(response.version(), reqwest::Version::HTTP_2);
    assert!(response.text().await.unwrap().contains("admin-a"));

    let ca = reqwest::Certificate::from_pem(&std::fs::read(&certs.ca).unwrap()).unwrap();
    let https = reqwest::Client::builder()
        .add_root_certificate(ca)
        .build()
        .unwrap();
    let response = https
        .get(format!("https://localhost:{}/", secure.port()))
        .send()
        .await
        .unwrap();
    assert_eq!(response.version(), reqwest::Version::HTTP_2);
    assert!(response.text().await.unwrap().contains("admin-a"));

    let (status, _, body) = support::h3::get(quic, &certs.ca, "/").await;
    assert_eq!(status, 200);
    assert!(String::from_utf8(body).unwrap().contains("admin-a"));
}

#[tokio::test]
async fn signed_in_denied_by_default() {
    let setup = setup("signed-in", None, "", None);
    let mut server = start(&setup.config, setup.dir.path());
    let plain = first(&server.listening(1), "tcp", 0);
    let response = reqwest::get(format!("http://{plain}/")).await.unwrap();
    assert_eq!(response.status(), 401);
    assert!(!response.text().await.unwrap().contains("admin-a"));
}

#[tokio::test]
async fn php_mount_is_served_by_its_pool() {
    let setup = setup("public", None, "", None);
    let mut server = start(&setup.config, setup.dir.path());
    let plain = first(&server.listening(1), "tcp", 0);
    let response = reqwest::get(format!("http://{plain}/blog/")).await.unwrap();
    assert_eq!(response.status(), 503);
    assert_eq!(response.headers()["retry-after"], "30");
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target)
        } else {
            std::fs::copy(entry.path(), target).map(|_| ()).unwrap()
        }
    }
}

#[test]
fn refuses_inconsistent_set() {
    let dir = tempfile::tempdir().unwrap();
    let admin = dir.path().join("admin");
    copy_dir(&fixture("admin-a"), &admin);
    let manifest = std::fs::read_to_string(admin.join("speedyspec-release.json"))
        .unwrap()
        .replace(">=0.1", "^99");
    std::fs::write(admin.join("speedyspec-release.json"), manifest).unwrap();
    let setup = setup("public", None, "", Some(&admin));
    let (code, _, stderr) = run_to_end(&["run", "--config", setup.config.to_str().unwrap()]);
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains("admin 1.0.0 requires server ^99, active server is"),
        "{stderr}"
    );
}

#[test]
fn refuses_manifest_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let admin = dir.path().join("admin");
    copy_dir(&fixture("admin-a"), &admin);
    let manifest = std::fs::read_to_string(admin.join("speedyspec-release.json"))
        .unwrap()
        .replace("\"app\": \"admin\"", "\"app\": \"other\"");
    std::fs::write(admin.join("speedyspec-release.json"), manifest).unwrap();
    let setup = setup("public", None, "", Some(&admin));
    let (code, _, stderr) = run_to_end(&["run", "--config", setup.config.to_str().unwrap()]);
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains("speedyspec-release.json") && stderr.contains("app"),
        "{stderr}"
    );
}

#[test]
fn refuses_mixed_mode() {
    let setup = setup("public", None, "", None);
    let toml = std::fs::read_to_string(&setup.config).unwrap().replace(
        "kind = \"static\"",
        "kind = \"static\"\nsource = \"https://x\"",
    );
    std::fs::write(&setup.config, toml).unwrap();
    let (code, _, stderr) = run_to_end(&["run", "--config", setup.config.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(
        stderr.starts_with("speedyspec-server: ") && stderr.contains("source"),
        "{stderr}"
    );
}

#[test]
fn refuses_dev_allow_all_on_public_address() {
    let setup = setup("public", Some("dev-allow-all"), "", None);
    let toml = std::fs::read_to_string(&setup.config)
        .unwrap()
        .replace("127.0.0.1:0", "0.0.0.0:0");
    std::fs::write(&setup.config, toml).unwrap();
    let (code, _, stderr) = run_to_end(&["run", "--config", setup.config.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(stderr.contains("loopback"), "{stderr}");
}

fn listing(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(listing(&path));
        }
        out.push(path);
    }
    out.sort();
    out
}

fn chmod(dir: &Path, mode: &str) {
    assert!(
        Command::new("chmod")
            .arg("-R")
            .arg(mode)
            .arg(dir)
            .status()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn writes_nothing() {
    let apps = tempfile::tempdir().unwrap();
    let admin = apps.path().join("admin");
    copy_dir(&fixture("admin-a"), &admin);
    let cwd = tempfile::tempdir().unwrap();
    let setup = setup("public", None, "", Some(&admin));
    let run_dir = cwd.path().join("run");
    let toml = std::fs::read_to_string(&setup.config).unwrap().replace(
        "mode = \"immutable\"",
        &format!("mode = \"immutable\"\nrun_dir = \"{}\"", run_dir.display()),
    );
    std::fs::write(&setup.config, toml).unwrap();
    let before = (listing(apps.path()), listing(cwd.path()));
    chmod(apps.path(), "a-w");
    chmod(cwd.path(), "a-w");

    let mut server = start(&setup.config, cwd.path());
    let plain = first(&server.listening(1), "tcp", 0);
    let response = reqwest::get(format!("http://{plain}/")).await.unwrap();
    assert_eq!(response.status(), 200);
    let status = server.term();
    let rest: Vec<String> = server.lines.try_iter().collect();
    server.log.extend(rest);
    assert!(
        server
            .log
            .iter()
            .any(|line| line.contains("\"event\":\"control_unavailable\"")),
        "{:?}",
        server.log
    );

    chmod(apps.path(), "u+w");
    chmod(cwd.path(), "u+w");
    assert!(status.success());
    assert_eq!((listing(apps.path()), listing(cwd.path())), before);
}

#[tokio::test]
async fn sigterm_exits_cleanly() {
    let setup = setup("public", None, "", None);
    let mut server = start(&setup.config, setup.dir.path());
    server.listening(1);
    assert_eq!(server.term().code(), Some(0));
}

#[test]
fn manifest_and_version() {
    let (code, stdout, _) = run_to_end(&["manifest"]);
    assert_eq!(code, 0);
    let manifest: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(manifest["kind"], "server");
    assert_eq!(manifest["app"], "server");
    assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));

    let (code, stdout, _) = run_to_end(&["--version"]);
    assert_eq!(code, 0);
    assert_eq!(
        stdout.trim(),
        format!("speedyspec-server {}", env!("CARGO_PKG_VERSION"))
    );

    let (code, _, stderr) = run_to_end(&[]);
    assert_eq!(code, 2);
    assert!(stderr.contains("Usage"), "{stderr}");
}

/// The README's example configurations parse, so the operator docs cannot drift from the config keys: the main
/// example, and the main example with the `[cluster]` example added.
#[test]
fn readme_example_config_parses() {
    let readme = include_str!("../../../README.md");
    let blocks: Vec<&str> = readme
        .split("```toml\n")
        .skip(1)
        .filter_map(|rest| rest.split("```").next())
        .collect();
    assert_eq!(
        blocks.len(),
        2,
        "the main example and the [cluster] example"
    );
    let config =
        speedyspec_server::config::Config::parse(blocks[0]).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(config.listeners.len(), 2);
    assert!(config.listeners[1].http3);
    assert_eq!(config.handoff.trial_period, Duration::from_secs(120));
    let config = speedyspec_server::config::Config::parse(&format!("{}\n{}", blocks[0], blocks[1]))
        .unwrap_or_else(|err| panic!("{err}"));
    let cluster = config.cluster.expect("the [cluster] example");
    assert_eq!(cluster.pod, "web-0");
    assert_eq!(cluster.probe, Some("0.0.0.0:9090".parse().unwrap()));
}
