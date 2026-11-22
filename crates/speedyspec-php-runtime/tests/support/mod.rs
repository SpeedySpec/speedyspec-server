//! Starting a worker and sending it requests, shared by the runtime's process tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use bytes::Bytes;
use speedyspec_fastcgi::{Client, Event};

pub const BIN: &str = env!("CARGO_BIN_EXE_speedyspec-php-runtime");

/// A running worker; killed when dropped.
pub struct Worker {
    pub child: Child,
    pub socket: PathBuf,
}

/// What one request produced: the stdout bytes (status line and headers included) and every stderr record.
pub struct Answer {
    pub stdout: String,
    pub stderr: Vec<Bytes>,
}

impl Answer {
    /// The `Status:` the worker sent, or 200 when PHP sent none.
    pub fn status(&self) -> u16 {
        self.stdout
            .lines()
            .take_while(|line| !line.trim().is_empty())
            .find_map(|line| line.strip_prefix("Status: "))
            .and_then(|rest| rest.split_whitespace().next()?.parse().ok())
            .unwrap_or(200)
    }

    /// The body, after the header block.
    pub fn body(&self) -> &str {
        self.stdout
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .unwrap_or(&self.stdout)
    }
}

/// The spec JSON for a worker.
pub fn spec(
    socket: &Path,
    release: &Path,
    site: Option<&Path>,
    env: &[(String, String)],
) -> String {
    serde_json::json!({
        "contract": 1, "app": "test", "socket": socket, "release_dir": release, "site_dir": site,
        "ini": {}, "bindings": [], "env": env
    })
    .to_string()
}

/// Spawns a worker without waiting for it.
pub fn spawn(dir: &Path, release: &Path, site: Option<&Path>, env: &[(String, String)]) -> Worker {
    let socket = dir.join(format!("w{}.sock", std::process::id() ^ rand_suffix()));
    let child = Command::new(BIN)
        .args(["worker", "--spec", &spec(&socket, release, site, env)])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    Worker { child, socket }
}

fn rand_suffix() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos()
}

impl Worker {
    /// Waits until the worker answers FastCGI management requests.
    pub async fn ready(&mut self) {
        let started = Instant::now();
        while Client::get_values(&self.socket, &["FCGI_MAX_CONNS"])
            .await
            .is_err()
        {
            if let Ok(Some(status)) = self.child.try_wait() {
                panic!("the worker exited: {status}: {}", self.stderr_text());
            }
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "the worker never answered"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// The worker's own stderr so far (only once it has exited).
    pub fn stderr_text(&mut self) -> String {
        let mut text = String::new();
        if let Some(mut err) = self.child.stderr.take() {
            std::io::Read::read_to_string(&mut err, &mut text).ok();
        }
        text
    }

    /// Sends a GET for `script` and collects the answer.
    pub async fn get(&self, script: &Path, uri: &str) -> Answer {
        let params = vec![
            (b"REQUEST_METHOD".to_vec(), b"GET".to_vec()),
            (
                b"SCRIPT_FILENAME".to_vec(),
                script.to_str().unwrap().as_bytes().to_vec(),
            ),
            (b"REQUEST_URI".to_vec(), uri.as_bytes().to_vec()),
        ];
        let body = futures_util::stream::empty::<std::io::Result<Bytes>>();
        let mut response = Client::request(&self.socket, params, body).await.unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        while let Some(event) = response.next().await {
            match event.unwrap() {
                Event::Stdout(bytes) => stdout.extend_from_slice(&bytes),
                Event::Stderr(bytes) => stderr.push(bytes),
                Event::End(_) => {}
            }
        }
        Answer {
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr,
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

/// `DB_*` variables for the test MySQL (`SPEEDYSPEC_TEST_MYSQL_URL`, `mysql://user:password@host:port/name`), with
/// `prefix` as `TABLE_PREFIX`; `None` when the variable is unset.
pub fn mysql_env(prefix: &str) -> Option<Vec<(String, String)>> {
    let url = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL").ok()?;
    let rest = url.strip_prefix("mysql://")?;
    let (auth, rest) = rest.rsplit_once('@')?;
    let (user, password) = auth.split_once(':').unwrap_or((auth, ""));
    let (host, name) = rest.split_once('/')?;
    Some(vec![
        ("DB_NAME".into(), name.into()),
        ("DB_USER".into(), user.into()),
        ("DB_PASSWORD".into(), password.into()),
        ("DB_HOST".into(), host.into()),
        ("DB_CHARSET".into(), "utf8mb4".into()),
        ("TABLE_PREFIX".into(), prefix.into()),
    ])
}

/// A release directory holding `index.php` with `code`.
pub fn release(dir: &Path, name: &str, code: &str) -> PathBuf {
    let release = dir.join(name);
    std::fs::create_dir_all(&release).unwrap();
    std::fs::write(release.join("index.php"), code).unwrap();
    release
}
