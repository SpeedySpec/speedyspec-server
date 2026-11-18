//! Running the real binary in tests.

use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

pub const BIN: &str = env!("CARGO_BIN_EXE_speedyspec-server");

pub struct Running {
    pub child: Child,
    lines: mpsc::Receiver<String>,
    pub log: Arc<Mutex<Vec<String>>>,
}

pub fn start(config: &Path) -> Running {
    let mut child = Command::new(BIN)
        .arg("run")
        .arg("--config")
        .arg(config)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, lines) = mpsc::channel();
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let all = log.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            all.lock().unwrap().push(line.clone());
            let _ = tx.send(line);
        }
    });
    Running { child, lines, log }
}

impl Running {
    /// The first TCP address the server reports listening on.
    pub fn address(&mut self) -> SocketAddr {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self
                .lines
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("no listening line: {:?}", self.log()));
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line)
                && value["event"] == "listening"
                && value["protocol"] == "tcp"
            {
                return value["address"].as_str().unwrap().parse().unwrap();
            }
        }
    }

    /// Waits for `count` `listening` events: (protocol, address).
    pub fn listening(&mut self, count: usize) -> Vec<(String, SocketAddr)> {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut found = Vec::new();
        while found.len() < count {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self
                .lines
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("no listening lines: {:?}", self.log()));
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line)
                && value["event"] == "listening"
            {
                found.push((
                    value["protocol"].as_str().unwrap().to_string(),
                    value["address"].as_str().unwrap().parse().unwrap(),
                ));
            }
        }
        found
    }

    pub fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    pub fn has_event(&self, event: &str) -> bool {
        self.log().iter().any(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .is_ok_and(|value| value["event"] == event)
        })
    }

    pub fn term(&mut self) {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(self.child.id().to_string())
            .status();
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "server did not stop");
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

/// Polls `GET http://<addr><path>` until `done(status, body)` or `timeout`.
pub fn poll(
    addr: SocketAddr,
    path: &str,
    timeout: Duration,
    done: impl Fn(u16, &str) -> bool,
) -> (u16, String) {
    let deadline = Instant::now() + timeout;
    loop {
        let (status, body) = get(addr, path);
        if done(status, &body) || Instant::now() >= deadline {
            return (status, body);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A blocking HTTP/1.1 GET with a fresh connection.
pub fn get(addr: SocketAddr, path: &str) -> (u16, String) {
    use std::io::{Read, Write};

    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    let status = raw
        .split_whitespace()
        .nth(1)
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    let body = raw
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    (status, body)
}

/// Runs `speedyspec-server <args>` to completion: (exit code, stdout, stderr).
pub fn command(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(BIN).args(args).output().unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}
