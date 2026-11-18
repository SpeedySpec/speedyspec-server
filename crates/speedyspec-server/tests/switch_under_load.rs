//! Switch under load (spec, Testing): requests over HTTP/1.1, h2c, HTTPS/h2 and HTTP/3 never fail and never mix
//! releases while a static app switches back and forth.

mod support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use speedyspec_server::control::{ControlRequest, ControlResponse, request};
use speedyspec_server_domain::AppId;
use support::release_host::ReleaseHost;
use support::run::{command, start};

const STAMPS: [&str; 2] = ["admin-1.0.0", "admin-1.1.0"];

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn switch_under_load() {
    let dir = tempfile::tempdir().unwrap();
    let certs = support::certs::generate(dir.path());
    let host = ReleaseHost::start();
    host.publish_static("admin", "1.0.0");
    host.publish_static("admin", "1.1.0");
    let config = dir.path().join("server.toml");
    std::fs::write(
        &config,
        format!(
            r#"mode = "managed"
data_dir = "{data}"
run_dir = "{run}"

[[listener]]
address = "127.0.0.1:0"

[[listener]]
address = "127.0.0.1:0"
tls = {{ cert = "{cert}", key = "{key}" }}
http3 = true

[app.server]
kind = "server"

[app.admin]
kind = "static"
source = "{source}"
keys = ["{public}"]
update = "manual"

[[mount]]
path = "/"
app = "admin"
"#,
            data = dir.path().join("data").display(),
            run = dir.path().join("run").display(),
            cert = certs.tls.cert.display(),
            key = certs.tls.key.display(),
            source = host.url("/admin/index.json"),
            public = host.public_key(),
        ),
    )
    .unwrap();
    let mut server = start(&config);
    let found = server.listening(3);
    let tcp: Vec<_> = found
        .iter()
        .filter(|(p, _)| p == "tcp")
        .map(|(_, a)| *a)
        .collect();
    let quic = found.iter().find(|(p, _)| p == "udp").unwrap().1;
    let (plain, secure) = (tcp[0], tcp[1]);
    let socket = dir.path().join("run/control.sock");
    let cfg = config.to_str().unwrap();
    for version in ["1.0.0", "1.1.0"] {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let (code, _, stderr) =
                command(&["release", "install", "--config", cfg, "admin", version]);
            if code == 0 {
                break;
            }
            assert!(Instant::now() < deadline, "{stderr}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    assert_eq!(
        command(&["release", "switch", "--config", cfg, "admin@1.0.0"]).0,
        0
    );

    let stop = Arc::new(AtomicBool::new(false));
    let failures: Arc<Mutex<Vec<String>>> = Arc::default();
    let counts: Arc<Mutex<[usize; 4]>> = Arc::default();
    let ca = reqwest::Certificate::from_pem(&std::fs::read(&certs.ca).unwrap()).unwrap();
    let clients = [
        (
            format!("http://{plain}"),
            reqwest::Client::builder().http1_only().build().unwrap(),
        ),
        (
            format!("http://{plain}"),
            reqwest::Client::builder()
                .http2_prior_knowledge()
                .build()
                .unwrap(),
        ),
        (
            format!("https://localhost:{}", secure.port()),
            reqwest::Client::builder()
                .add_root_certificate(ca)
                .build()
                .unwrap(),
        ),
    ];
    let check = |protocol: &str, status: u16, body: &str, failures: &Mutex<Vec<String>>| {
        if status != 200 || !STAMPS.contains(&body) {
            failures
                .lock()
                .unwrap()
                .push(format!("{protocol}: {status} {body:?}"));
        }
    };
    let mut tasks = Vec::new();
    for (index, (base, client)) in clients.into_iter().enumerate() {
        let (stop, failures, counts) = (stop.clone(), failures.clone(), counts.clone());
        tasks.push(tokio::spawn(async move {
            while !stop.load(Ordering::SeqCst) {
                for path in ["/", "/_astro/app.js"] {
                    match client.get(format!("{base}{path}")).send().await {
                        Ok(response) => {
                            let status = response.status().as_u16();
                            let body = response.text().await.unwrap_or_default();
                            check(&format!("client {index}"), status, &body, &failures);
                        }
                        Err(err) => failures
                            .lock()
                            .unwrap()
                            .push(format!("client {index}: {err}")),
                    }
                    counts.lock().unwrap()[index] += 1;
                }
            }
        }));
    }
    {
        let (stop, failures, counts, ca) = (
            stop.clone(),
            failures.clone(),
            counts.clone(),
            certs.ca.clone(),
        );
        tasks.push(tokio::spawn(async move {
            while !stop.load(Ordering::SeqCst) {
                for path in ["/", "/_astro/app.js"] {
                    let (status, _, body) = support::h3::get(quic, &ca, path).await;
                    check("h3", status, &String::from_utf8_lossy(&body), &failures);
                    counts.lock().unwrap()[3] += 1;
                }
            }
        }));
    }

    let switcher = std::thread::spawn(move || {
        let started = Instant::now();
        let mut switches = 0;
        while started.elapsed() < Duration::from_secs(5) {
            let version = if switches % 2 == 0 { "1.1.0" } else { "1.0.0" };
            let target = (AppId::parse("admin").unwrap(), version.parse().unwrap());
            match request(
                &socket,
                &ControlRequest::Switch {
                    targets: vec![target],
                },
            ) {
                Ok(ControlResponse::Ok(_)) => switches += 1,
                other => panic!("switch failed: {other:?}"),
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        switches
    });
    let switches = tokio::task::spawn_blocking(move || switcher.join().unwrap())
        .await
        .unwrap();
    stop.store(true, Ordering::SeqCst);
    for task in tasks {
        task.await.unwrap();
    }

    let failures = failures.lock().unwrap();
    assert!(
        failures.is_empty(),
        "{} failed requests, first: {:?}",
        failures.len(),
        &failures[..failures.len().min(5)]
    );
    let counts = *counts.lock().unwrap();
    assert!(
        counts.iter().all(|c| *c > 0),
        "every protocol served requests: {counts:?}"
    );
    assert!(switches >= 10, "{switches} switches");
    drop(server);
}
