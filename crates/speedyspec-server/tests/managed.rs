//! Managed mode end to end: a release host, the real binary, a temp data and run directory.

mod support;

use std::path::{Path, PathBuf};
use std::time::Duration;

use support::release_host::ReleaseHost;
use support::run::{get, poll, start};

/// Writes a managed config for the `admin` static app mounted at `/`, with `extra` appended to `[app.admin]`.
fn config(dir: &Path, host: &ReleaseHost, admin_extra: &str) -> PathBuf {
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
{admin_extra}

[[mount]]
path = "/"
app = "admin"
"#,
        data = dir.join("data").display(),
        run = dir.join("run").display(),
        source = host.url("/admin/index.json"),
    );
    let path = dir.join("server.toml");
    std::fs::write(&path, toml).unwrap();
    path
}

fn signed(host: &ReleaseHost) -> String {
    format!("keys = [\"{}\"]", host.public_key())
}

#[test]
fn first_start_installs_auto_app() {
    let dir = tempfile::tempdir().unwrap();
    let host = ReleaseHost::start();
    host.publish_static("admin", "1.0.0");
    let mut server = start(&config(dir.path(), &host, &signed(&host)));
    let addr = server.address();
    let (status, body) = poll(addr, "/", Duration::from_secs(10), |status, _| {
        status != 503
    });
    assert_eq!(status, 200, "log: {:?}", server.log());
    assert_eq!(body, "admin-1.0.0");
    assert!(
        // The app serves as soon as it is activated; `switched` is logged after the state commits, so it can trail
        // the first 200.
        server.wait_for_event("installed", Duration::from_secs(5))
            && server.wait_for_event("switched", Duration::from_secs(5)),
        "{:?}",
        server.log()
    );
}

#[test]
fn not_installed_answers_503() {
    let dir = tempfile::tempdir().unwrap();
    let host = ReleaseHost::start();
    let mut server = start(&config(
        dir.path(),
        &host,
        &format!("{}\nupdate = \"manual\"", signed(&host)),
    ));
    let addr = server.address();
    let raw = {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        write!(
            stream,
            "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        raw
    };
    assert!(raw.starts_with("HTTP/1.1 503"), "{raw}");
    assert!(
        raw.to_ascii_lowercase().contains("retry-after: 30"),
        "{raw}"
    );
}

#[test]
fn manual_app_waits() {
    let dir = tempfile::tempdir().unwrap();
    let host = ReleaseHost::start();
    host.publish_static("admin", "1.0.0");
    let mut server = start(&config(
        dir.path(),
        &host,
        &format!("{}\nupdate = \"manual\"", signed(&host)),
    ));
    let addr = server.address();
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(get(addr, "/").0, 503);
    assert!(host.requests().is_empty(), "{:?}", host.requests());
}

#[test]
fn restart_serves_last_active() {
    let dir = tempfile::tempdir().unwrap();
    let host = ReleaseHost::start();
    host.publish_static("admin", "1.0.0");
    let config = config(dir.path(), &host, &signed(&host));
    let mut server = start(&config);
    let addr = server.address();
    assert_eq!(
        poll(addr, "/", Duration::from_secs(10), |s, _| s == 200).1,
        "admin-1.0.0"
    );
    server.term();
    host.stop();
    let mut again = start(&config);
    let addr = again.address();
    assert_eq!(
        poll(addr, "/", Duration::from_secs(5), |s, _| s == 200),
        (200, "admin-1.0.0".to_string())
    );
}

#[test]
fn unsigned_warning_logged() {
    let dir = tempfile::tempdir().unwrap();
    let host = ReleaseHost::start();
    let mut server = start(&config(
        dir.path(),
        &host,
        "allow_unsigned = true\nupdate = \"manual\"",
    ));
    server.address();
    assert!(
        server.wait_for_event("unsigned", Duration::from_secs(5)),
        "{:?}",
        server.log()
    );
}

#[test]
fn bad_signature_is_refused_and_logged() {
    let dir = tempfile::tempdir().unwrap();
    let host = ReleaseHost::start();
    host.publish_static("admin", "1.0.0");
    let other = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    let mut server = start(&config(
        dir.path(),
        &host,
        &format!("keys = [\"{}\"]", other.pk.to_base64()),
    ));
    let addr = server.address();
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(get(addr, "/").0, 503);
    assert!(
        server.wait_for_event("auto_update_failed", Duration::from_secs(5)),
        "{:?}",
        server.log()
    );
}
