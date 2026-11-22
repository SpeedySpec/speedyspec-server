//! The runtime guarantees ext-sodium and serves WordPress's sodium functions from Rust.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use bytes::Bytes;
use speedyspec_fastcgi::{Client, Event};
use speedyspec_runtime_contract::RuntimeManifest;

const BIN: &str = env!("CARGO_BIN_EXE_speedyspec-php-runtime");

#[test]
fn manifest_reports_sodium() {
    let output = Command::new(BIN).arg("manifest").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest: RuntimeManifest = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(manifest.sodium.as_deref(), Some("1.0.22"));
}

#[tokio::test(flavor = "current_thread")]
async fn worker_serves_the_port() {
    let dir = tempfile::tempdir().unwrap();
    let release = dir.path().join("release");
    std::fs::create_dir(&release).unwrap();
    std::fs::write(
        release.join("index.php"),
        "<?php echo wp_fast_hash('abc'), '|', (new ReflectionFunction('wp_fast_hash'))->getFileName(), '|',
         var_export(wp_verify_fast_hash('abc', wp_fast_hash('abc')), true);",
    )
    .unwrap();
    let socket = dir.path().join("w.sock");
    let spec = serde_json::json!({
        "contract": 1, "app": "test", "socket": socket, "release_dir": release, "site_dir": null,
        "ini": {}, "bindings": [], "env": []
    });
    let mut worker = Command::new(BIN)
        .args(["worker", "--spec", &spec.to_string()])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while Client::get_values(&socket, &["FCGI_MAX_CONNS"])
        .await
        .is_err()
    {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the worker never answered"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let script = release.join("index.php");
    let params = vec![
        (b"REQUEST_METHOD".to_vec(), b"GET".to_vec()),
        (
            b"SCRIPT_FILENAME".to_vec(),
            script.to_str().unwrap().as_bytes().to_vec(),
        ),
        (b"REQUEST_URI".to_vec(), b"/".to_vec()),
    ];
    let body = futures_util::stream::empty::<std::io::Result<Bytes>>();
    let mut response = Client::request(Path::new(&socket), params, body)
        .await
        .unwrap();
    let mut stdout = Vec::new();
    while let Some(event) = response.next().await {
        if let Event::Stdout(bytes) = event.unwrap() {
            stdout.extend_from_slice(&bytes);
        }
    }
    worker.kill().ok();
    worker.wait().ok();
    let text = String::from_utf8_lossy(&stdout);
    assert!(
        text.ends_with(
            "$generic$YeaPr_itxnUOyFBBm16EGFnt_nzqpbL728cakJ4U|speedyspec://wp-crypto|true"
        ),
        "{text}"
    );
}
