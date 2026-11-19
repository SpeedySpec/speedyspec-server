//! A release host for tests: serves signed static releases and their index over plain HTTP on loopback.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};
use speedyspec_server_infra_memory::archive::release;

pub struct ReleaseHost {
    addr: SocketAddr,
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    requests: Arc<Mutex<Vec<String>>>,
    stopped: Arc<AtomicBool>,
    key: minisign::KeyPair,
    published: Mutex<Vec<(String, String, String)>>,
    servers: Mutex<Vec<(String, String)>>,
}

impl ReleaseHost {
    pub fn start() -> ReleaseHost {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let files: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::default();
        let requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let stopped = Arc::new(AtomicBool::new(false));
        let (f, r, s) = (files.clone(), requests.clone(), stopped.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if s.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let (f, r) = (f.clone(), r.clone());
                std::thread::spawn(move || serve(stream, &f, &r));
            }
        });
        let key = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        ReleaseHost {
            addr,
            files,
            requests,
            stopped,
            key,
            published: Mutex::new(Vec::new()),
            servers: Mutex::new(Vec::new()),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    /// The base64 public key to put in `app.<id>.keys`.
    pub fn public_key(&self) -> String {
        self.key.pk.to_base64()
    }

    /// Publishes a signed static release of `app` whose `dist/index.html` and `dist/_astro/app.js` both read
    /// `<app>-<version>`, and rewrites the app's index.
    pub fn publish_static(&self, app: &str, version: &str) {
        self.publish_static_requiring(app, version, "");
    }

    /// Like `publish_static`, with `requires` as the inside of the manifest's `requires` object.
    pub fn publish_static_requiring(&self, app: &str, version: &str, requires: &str) {
        let manifest = format!(
            r#"{{"app":"{app}","version":"{version}","kind":"static","requires":{{{requires}}},"static":{{"root":"dist","cache":[{{"match":"_astro/**","cache_control":"public, max-age=31536000, immutable"}}]}}}}"#
        );
        let stamp = format!("{app}-{version}");
        self.publish_release(
            app,
            version,
            &manifest,
            &[("dist/index.html", &stamp), ("dist/_astro/app.js", &stamp)],
        );
    }

    /// Publishes a signed release of `app` with `manifest` and `files`, and rewrites the app's index.
    pub fn publish_release(
        &self,
        app: &str,
        version: &str,
        manifest: &str,
        files: &[(&str, &str)],
    ) {
        let bytes = release(manifest, files);
        let sha: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let signature = minisign::sign(
            Some(&self.key.pk),
            &self.key.sk,
            std::io::Cursor::new(&bytes),
            None,
            None,
        )
        .unwrap();
        let path = format!("/{app}/{app}-{version}.tar.zst");
        let mut files = self.files.lock().unwrap();
        files.insert(path.clone(), bytes);
        files.insert(
            format!("{path}.minisig"),
            signature.into_string().into_bytes(),
        );
        let mut published = self.published.lock().unwrap();
        published.push((app.to_string(), version.to_string(), sha));
        let entries: Vec<String> = published
            .iter()
            .filter(|(a, _, _)| a == app)
            .map(|(a, v, sha)| {
                let url = self.url(&format!("/{a}/{a}-{v}.tar.zst"));
                format!(
                    r#"{{"version":"{v}","published":"x","artifacts":[{{"target":"any","url":"{url}","sha256":"{sha}","minisig":"{url}.minisig"}}]}}"#
                )
            })
            .collect();
        files.insert(
            format!("/{app}/index.json"),
            format!(r#"{{"app":"{app}","releases":[{}]}}"#, entries.join(",")).into_bytes(),
        );
    }

    /// Publishes `binary` as server release `version` for this build's target triple.
    pub fn publish_server(&self, version: &str, binary: &std::path::Path) {
        self.publish_server_for(version, binary, speedyspec_server::binary::TARGET);
    }

    /// Publishes `binary` as server release `version` for `target` (another platform's binary, for a container).
    pub fn publish_server_for(&self, version: &str, binary: &std::path::Path, target: &str) {
        let bytes = std::fs::read(binary).unwrap();
        let sha: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let signature = minisign::sign(
            Some(&self.key.pk),
            &self.key.sk,
            std::io::Cursor::new(&bytes),
            None,
            None,
        )
        .unwrap();
        let path = format!("/server/{target}/server-{version}");
        let mut files = self.files.lock().unwrap();
        files.insert(path.clone(), bytes);
        files.insert(
            format!("{path}.minisig"),
            signature.into_string().into_bytes(),
        );
        let mut published = self.servers.lock().unwrap();
        published.push((version.to_string(), sha));
        let entries: Vec<String> = published
            .iter()
            .map(|(v, sha)| {
                let url = self.url(&format!("/server/{target}/server-{v}"));
                format!(
                    r#"{{"version":"{v}","published":"x","artifacts":[{{"target":"{target}","url":"{url}","sha256":"{sha}","minisig":"{url}.minisig"}}]}}"#
                )
            })
            .collect();
        files.insert(
            "/server/index.json".to_string(),
            format!(r#"{{"app":"server","releases":[{}]}}"#, entries.join(",")).into_bytes(),
        );
    }

    /// Writes every published file under `dir`, with this host's URL in the indexes replaced by `base` (where
    /// another server will serve the same files).
    pub fn write_to(&self, dir: &std::path::Path, base: &str) {
        let own = format!("http://{}", self.addr);
        for (path, bytes) in self.files.lock().unwrap().iter() {
            let target = dir.join(path.trim_start_matches('/'));
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            let bytes = if path.ends_with(".json") {
                String::from_utf8(bytes.clone())
                    .unwrap()
                    .replace(&own, base)
                    .into_bytes()
            } else {
                bytes.clone()
            };
            std::fs::write(target, bytes).unwrap();
        }
    }

    /// Paths requested so far.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
    }
}

impl Drop for ReleaseHost {
    fn drop(&mut self) {
        self.stop();
    }
}

fn serve(
    stream: TcpStream,
    files: &Mutex<HashMap<String, Vec<u8>>>,
    requests: &Mutex<Vec<String>>,
) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || header == "\r\n" || header.is_empty() {
            break;
        }
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
    requests.lock().unwrap().push(path.clone());
    let body = files.lock().unwrap().get(&path).cloned();
    let mut stream = stream;
    let response = match body {
        Some(body) => {
            let mut out = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            out.extend_from_slice(&body);
            out
        }
        None => {
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        }
    };
    let _ = stream.write_all(&response);
}
