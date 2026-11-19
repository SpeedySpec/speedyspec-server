//! Switching the PHP runtime moves every PHP app on it to new pools, in one step, from the runtime's new binary.

mod support;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use support::release_host::ReleaseHost;
use support::run::{Running, command, start};
use support::runtime::{Workers, script};

struct Site {
    _dir: tempfile::TempDir,
    config: String,
    host: ReleaseHost,
    server: Running,
}

fn get(addr: std::net::SocketAddr, path: &str) -> Option<String> {
    let response = reqwest::blocking::Client::new()
        .get(format!("http://{addr}{path}"))
        .timeout(Duration::from_secs(5))
        .send()
        .ok()?;
    (response.status() == 200).then(|| response.text().unwrap_or_default())
}

fn release_cmd(config: &str, args: &[&str]) {
    let mut all = vec!["release"];
    all.extend_from_slice(args);
    all.extend_from_slice(&["--config", config]);
    let (code, out, err) = command(&all);
    assert_eq!(code, 0, "{all:?}: {out}{err}");
}

fn publish_app(host: &ReleaseHost, app: &str, version: &str) {
    let manifest = format!(
        r#"{{"app":"{app}","version":"{version}","kind":"php","php":{{"runtime":"^1","health":{{"live":"/health.php","ready":"/health.php"}}}}}}"#
    );
    // The health probe fails on runtime 1.9.0 (a release a test uses as a bad runtime).
    let health = "<?php if (getenv('SPEEDYSPEC_RUNTIME_LABEL') === '1.9.0') { http_response_code(500); } echo 'ok';";
    let index = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime-app/index.php"),
    )
    .unwrap();
    host.publish_release(
        app,
        version,
        &manifest,
        &[
            ("index.php", &index),
            ("app.txt", &format!("{app}@{version}")),
            ("health.php", health),
        ],
    );
}

/// A managed server with runtime `rt` 1.0.0 and the php apps `blog` (at `/blog/`) and `frontend-api` (at `/api/`)
/// on it, all installed and live.
fn site() -> Site {
    support::runtime::binary();
    let host = ReleaseHost::start();
    host.publish_binary("rt", "1.0.0", script("1.0.0", Workers::Real).as_bytes());
    publish_app(&host, "blog", "1.0.0");
    publish_app(&host, "frontend-api", "1.0.0");
    let dir = tempfile::tempdir().unwrap();
    let mut apps = String::new();
    for app in ["rt", "blog", "frontend-api"] {
        let kind = if app == "rt" { "runtime" } else { "php" };
        apps.push_str(&format!(
            "[app.{app}]\nkind = \"{kind}\"\nsource = \"{}\"\nkeys = [\"{}\"]\nupdate = \"manual\"\n",
            host.url(&format!("/{app}/index.json")),
            host.public_key()
        ));
        if kind == "php" {
            let site = dir.path().join(format!("site-{app}"));
            std::fs::create_dir_all(&site).unwrap();
            apps.push_str(&format!(
                "[app.{app}.php]\nsite_dir = \"{}\"\n",
                site.display()
            ));
        }
    }
    let toml = format!(
        "mode = \"managed\"\ndata_dir = \"{data}\"\nrun_dir = \"{run}\"\n\n[[listener]]\naddress = \"127.0.0.1:0\"\n\n[health]\ninterval = \"200ms\"\n\n[runtime]\ntrial_period = \"30s\"\n\n[app.server]\nkind = \"server\"\n\n{apps}\n[[mount]]\npath = \"/blog/\"\napp = \"blog\"\n\n[[mount]]\npath = \"/api/\"\napp = \"frontend-api\"\n",
        data = dir.path().join("data").display(),
        run = dir.path().join("run").display(),
    );
    let config = dir.path().join("server.toml");
    std::fs::write(&config, toml).unwrap();
    let server = start(&config);
    let keeper = dir.path().join("run/keeper.sock");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !keeper.exists() {
        assert!(Instant::now() < deadline, "no control socket");
        std::thread::sleep(Duration::from_millis(50));
    }
    let config = config.display().to_string();
    for app in ["rt", "blog", "frontend-api"] {
        release_cmd(&config, &["install", app, "1.0.0"]);
    }
    release_cmd(&config, &["switch", "rt@1.0.0"]);
    release_cmd(&config, &["switch", "blog@1.0.0", "frontend-api@1.0.0"]);
    Site {
        _dir: dir,
        config,
        host,
        server,
    }
}

fn wait_for(addr: std::net::SocketAddr, path: &str, want: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let got = get(addr, path);
        if got.as_deref() == Some(want) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{path} answered {got:?}, wanted {want:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn runtime_switch_moves_every_php_app() {
    let mut site = site();
    let addr = site.server.address();
    wait_for(addr, "/blog/", "1.0.0 blog@1.0.0");
    wait_for(addr, "/api/", "1.0.0 frontend-api@1.0.0");

    let stop = Arc::new(AtomicBool::new(false));
    let failures = Arc::new(AtomicU64::new(0));
    let load = {
        let (stop, failures) = (stop.clone(), failures.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                for path in ["/blog/", "/api/"] {
                    if get(addr, path).is_none() {
                        failures.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        })
    };
    site.host
        .publish_binary("rt", "1.1.0", script("1.1.0", Workers::Real).as_bytes());
    release_cmd(&site.config, &["install", "rt", "1.1.0"]);
    release_cmd(&site.config, &["switch", "rt@1.1.0"]);
    assert_eq!(get(addr, "/blog/").as_deref(), Some("1.1.0 blog@1.0.0"));
    assert_eq!(
        get(addr, "/api/").as_deref(),
        Some("1.1.0 frontend-api@1.0.0")
    );
    std::thread::sleep(Duration::from_millis(300));
    stop.store(true, Ordering::SeqCst);
    load.join().unwrap();
    assert_eq!(
        failures.load(Ordering::SeqCst),
        0,
        "no request failed during the switch"
    );
}

#[test]
fn runtime_that_fails_to_prepare_switches_nothing() {
    let mut site = site();
    let addr = site.server.address();
    wait_for(addr, "/blog/", "1.0.0 blog@1.0.0");
    site.host.publish_binary(
        "rt",
        "1.2.0",
        script("1.2.0", Workers::ExitAtOnce).as_bytes(),
    );
    release_cmd(&site.config, &["install", "rt", "1.2.0"]);
    let (code, out, err) = command(&["release", "switch", "rt@1.2.0", "--config", &site.config]);
    assert_ne!(code, 0, "{out}{err}");
    assert!(
        format!("{out}{err}").contains("blog") || format!("{out}{err}").contains("frontend-api"),
        "the error names an app that failed: {out}{err}"
    );
    assert_eq!(get(addr, "/blog/").as_deref(), Some("1.0.0 blog@1.0.0"));
    assert_eq!(
        get(addr, "/api/").as_deref(),
        Some("1.0.0 frontend-api@1.0.0")
    );
    let (_, list, _) = command(&["release", "list", "rt", "--config", &site.config]);
    let failed = list
        .lines()
        .find(|line| line.starts_with("rt\t"))
        .and_then(|line| line.split('\t').last());
    assert_eq!(failed, Some("1.2.0"), "1.2.0 is listed as failed: {list}");
}

#[test]
fn bad_runtime_rolls_back_on_its_own() {
    let mut site = site();
    let addr = site.server.address();
    wait_for(addr, "/blog/", "1.0.0 blog@1.0.0");
    site.host
        .publish_binary("rt", "1.9.0", script("1.9.0", Workers::Real).as_bytes());
    release_cmd(&site.config, &["install", "rt", "1.9.0"]);
    release_cmd(&site.config, &["switch", "rt@1.9.0"]);
    // The blog's probes fail on 1.9.0: within a few rounds the runtime goes back to 1.0.0 on its own.
    wait_for(addr, "/blog/", "1.0.0 blog@1.0.0");
    let (_, list, _) = command(&["release", "list", "rt", "--config", &site.config]);
    let row = list
        .lines()
        .find(|line| line.starts_with("rt\t"))
        .unwrap_or_default()
        .to_string();
    assert!(
        row.split('\t').nth(2) == Some("1.0.0") && row.ends_with("1.9.0"),
        "active 1.0.0, 1.9.0 failed: {list}"
    );
    assert!(
        site.server.has_event("runtime_trial_failed"),
        "{:?}",
        site.server.log()
    );
}
