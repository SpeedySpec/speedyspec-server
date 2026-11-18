//! A tiny stand-in for the Kubernetes API: one Lease, over plain HTTP on the loopback interface, with
//! `resourceVersion` conflicts and switches that make requests fail.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

#[derive(Default)]
struct State {
    lease: Option<Value>,
    version: u64,
    /// While set, every PUT answers 500.
    fail_updates: bool,
    /// The next PUT answers 409, whatever its resourceVersion.
    conflict_next: bool,
    puts: usize,
}

pub struct FakeApi {
    pub url: String,
    state: Arc<Mutex<State>>,
}

impl FakeApi {
    pub fn start() -> FakeApi {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state: Arc<Mutex<State>> = Arc::default();
        let shared = state.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let state = shared.clone();
                std::thread::spawn(move || serve(stream, &state));
            }
        });
        FakeApi { url, state }
    }

    pub fn lease(&self) -> Option<Value> {
        self.state.lock().unwrap().lease.clone()
    }

    pub fn fail_updates(&self, fail: bool) {
        self.state.lock().unwrap().fail_updates = fail;
    }

    pub fn conflict_next(&self) {
        self.state.lock().unwrap().conflict_next = true;
    }

    pub fn puts(&self) -> usize {
        self.state.lock().unwrap().puts
    }
}

fn serve(stream: TcpStream, state: &Mutex<State>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut writer = stream;
    loop {
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
            return;
        }
        let mut length = 0usize;
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).unwrap_or(0) == 0 {
                return;
            }
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some((name, value)) = header.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0; length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        let mut parts = request_line.split_whitespace();
        let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let path = path.split('?').next().unwrap_or("");
        let (status, reply) = answer(method, path, &body, state);
        let text = reply.to_string();
        let response = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{text}",
            text.len()
        );
        if writer.write_all(response.as_bytes()).is_err() {
            return;
        }
    }
}

fn status(code: u16, reason: &str) -> (u16, Value) {
    (
        code,
        json!({"kind": "Status", "apiVersion": "v1", "status": "Failure", "reason": reason, "code": code, "message": reason}),
    )
}

fn answer(method: &str, path: &str, body: &[u8], state: &Mutex<State>) -> (u16, Value) {
    let mut state = state.lock().unwrap();
    let collection = path.ends_with("/leases");
    match (method, collection) {
        ("GET", false) => match &state.lease {
            Some(lease) => (200, lease.clone()),
            None => status(404, "NotFound"),
        },
        ("POST", true) => {
            if state.lease.is_some() {
                return status(409, "AlreadyExists");
            }
            let mut lease: Value = serde_json::from_slice(body).unwrap();
            state.version += 1;
            lease["metadata"]["resourceVersion"] = json!(state.version.to_string());
            state.lease = Some(lease.clone());
            (201, lease)
        }
        ("PUT", false) => {
            state.puts += 1;
            if state.fail_updates {
                return status(500, "InternalError");
            }
            if std::mem::take(&mut state.conflict_next) {
                return status(409, "Conflict");
            }
            let mut lease: Value = serde_json::from_slice(body).unwrap();
            let current = state
                .lease
                .as_ref()
                .map(|l| l["metadata"]["resourceVersion"].clone());
            if current != Some(lease["metadata"]["resourceVersion"].clone()) {
                return status(409, "Conflict");
            }
            state.version += 1;
            lease["metadata"]["resourceVersion"] = json!(state.version.to_string());
            state.lease = Some(lease.clone());
            (200, lease)
        }
        _ => status(405, "MethodNotAllowed"),
    }
}
