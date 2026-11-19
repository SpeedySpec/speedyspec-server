//! Releases that migrate, end to end: under maintenance, before going live, safe to repeat on independent replicas.

mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use support::release_host::ReleaseHost;
use support::run::{Running, command, start};
use support::runtime::{Workers, script};

fn get(addr: std::net::SocketAddr, path: &str) -> (u16, String) {
    match reqwest::blocking::Client::new()
        .get(format!("http://{addr}{path}"))
        .timeout(Duration::from_secs(10))
        .send()
    {
        Ok(response) => (
            response.status().as_u16(),
            response.text().unwrap_or_default(),
        ),
        Err(_) => (0, String::new()),
    }
}

fn run(config: &str, args: &[&str]) -> (i32, String) {
    let mut all = args.to_vec();
    all.extend_from_slice(&["--config", config]);
    let (code, out, err) = command(&all);
    (code, format!("{out}{err}"))
}

fn ok(config: &str, args: &[&str]) {
    let (code, out) = run(config, args);
    assert_eq!(code, 0, "{args:?}: {out}");
}

/// The blog's migrate.php: takes a lock on the shared database directory, applies once, answers 204 when already
/// applied, and answers `fail` (a status) when the release says so.
const MIGRATE: &str = "<?php\n$db = getenv('SHARED_DB');\n$lock = fopen($db . '/lock', 'c');\nflock($lock, LOCK_EX);\nif (is_file(__DIR__ . '/fail')) { http_response_code((int) file_get_contents(__DIR__ . '/fail')); echo 'boom'; exit; }\nif (is_file($db . '/applied-' . VERSION)) { http_response_code(204); exit; }\nusleep(800000);\nfile_put_contents($db . '/applied-' . VERSION, '1');\nfile_put_contents($db . '/count', (string) ((int) @file_get_contents($db . '/count') + 1));\necho 'migrated';\n";

fn publish_blog(host: &ReleaseHost, version: &str, migrates: bool, fail: Option<u16>) {
    let switch = if migrates {
        r#","switch":{"maintenance":"self","migrate":"/migrate.php"}"#
    } else {
        ""
    };
    let manifest = format!(
        r#"{{"app":"blog","version":"{version}","kind":"php","php":{{"runtime":"^1"{switch}}}}}"#
    );
    let migrate = format!("<?php const VERSION = '{version}'; ?>{MIGRATE}");
    let mut files = vec![
        (
            "index.php".to_string(),
            format!("<?php echo 'v{version}';\n"),
        ),
        ("migrate.php".to_string(), migrate),
    ];
    if let Some(status) = fail {
        files.push(("fail".to_string(), status.to_string()));
    }
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    host.publish_release("blog", version, &manifest, &files);
}

struct Server {
    _dir: tempfile::TempDir,
    config: String,
    server: Running,
    addr: std::net::SocketAddr,
}

/// A managed server on `host` with runtime rt 1.0.0 and the blog at 1.0.0, live.
fn server(host: &ReleaseHost, shared_db: &Path) -> Server {
    let dir = tempfile::tempdir().unwrap();
    let site = dir.path().join("site");
    std::fs::create_dir_all(site.join("wp-content")).unwrap();
    let toml = format!(
        "mode = \"managed\"\ndata_dir = \"{data}\"\nrun_dir = \"{run}\"\n\n[[listener]]\naddress = \"127.0.0.1:0\"\n\n[app.server]\nkind = \"server\"\n\n[app.rt]\nkind = \"runtime\"\nsource = \"{rt}\"\nkeys = [\"{key}\"]\nupdate = \"manual\"\n\n[app.blog]\nkind = \"php\"\nsource = \"{blog}\"\nkeys = [\"{key}\"]\nupdate = \"manual\"\n[app.blog.php]\nsite_dir = \"{site}\"\nenv = {{ SHARED_DB = \"{db}\" }}\n\n[[mount]]\npath = \"/blog/\"\napp = \"blog\"\n",
        data = dir.path().join("data").display(),
        run = dir.path().join("run").display(),
        rt = host.url("/rt/index.json"),
        blog = host.url("/blog/index.json"),
        key = host.public_key(),
        site = site.display(),
        db = shared_db.display(),
    );
    let config = dir.path().join("server.toml");
    std::fs::write(&config, toml).unwrap();
    let mut server = start(&config);
    let addr = server.address();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !dir.path().join("run/keeper.sock").exists() {
        assert!(Instant::now() < deadline, "no control socket");
        std::thread::sleep(Duration::from_millis(50));
    }
    let config = config.display().to_string();
    for args in [
        vec!["release", "install", "rt", "1.0.0"],
        vec!["release", "switch", "rt@1.0.0"],
        vec!["release", "install", "blog", "1.0.0"],
        vec!["release", "switch", "blog@1.0.0"],
    ] {
        ok(&config, &args);
    }
    Server {
        _dir: dir,
        config,
        server,
        addr,
    }
}

fn host() -> ReleaseHost {
    support::runtime::binary();
    let host = ReleaseHost::start();
    host.publish_binary("rt", "1.0.0", script("1.0.0", Workers::Real).as_bytes());
    publish_blog(&host, "1.0.0", false, None);
    host
}

#[test]
fn a_migrating_switch_is_under_maintenance_then_live() {
    let host = host();
    publish_blog(&host, "2.0.0", true, None);
    let db = tempfile::tempdir().unwrap();
    let s = server(&host, db.path());
    assert_eq!(get(s.addr, "/blog/"), (200, "v1.0.0".into()));
    ok(&s.config, &["release", "install", "blog", "2.0.0"]);
    let config = s.config.clone();
    let switching = std::thread::spawn(move || run(&config, &["release", "switch", "blog@2.0.0"]));
    let mut saw_maintenance = false;
    let deadline = Instant::now() + Duration::from_secs(20);
    while !switching.is_finished() && Instant::now() < deadline {
        if get(s.addr, "/blog/").0 == 503 {
            saw_maintenance = true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let (code, out) = switching.join().unwrap();
    assert_eq!(code, 0, "{out}");
    assert!(
        saw_maintenance,
        "the blog was in maintenance while it migrated"
    );
    assert_eq!(get(s.addr, "/blog/"), (200, "v2.0.0".into()));
    assert!(db.path().join("applied-2.0.0").exists());
    assert_eq!(
        get(s.addr, "/blog/migrate.php").0,
        404,
        "the migration path is internal"
    );
    assert!(s.server.has_event("migrating"), "{:?}", s.server.log());
}

#[test]
fn a_failed_migration_holds_maintenance_until_cleared() {
    let host = host();
    publish_blog(&host, "3.0.0", true, Some(500));
    let db = tempfile::tempdir().unwrap();
    let s = server(&host, db.path());
    ok(&s.config, &["release", "install", "blog", "3.0.0"]);
    let (code, out) = run(&s.config, &["release", "switch", "blog@3.0.0"]);
    assert_ne!(code, 0, "{out}");
    assert!(
        out.contains("migration failed") && out.contains("500"),
        "{out}"
    );
    assert_eq!(get(s.addr, "/blog/").0, 503, "maintenance stays on");
    ok(&s.config, &["maintenance", "off", "blog"]);
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(
        get(s.addr, "/blog/").0,
        503,
        "off clears only the operator's own maintenance"
    );
    ok(&s.config, &["maintenance", "clear", "blog"]);
    let deadline = Instant::now() + Duration::from_secs(5);
    while get(s.addr, "/blog/") != (200, "v1.0.0".to_string()) {
        assert!(Instant::now() < deadline, "the old release never came back");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn independent_replicas_each_migrate_safely() {
    let host = host();
    publish_blog(&host, "2.0.0", true, None);
    let db = tempfile::tempdir().unwrap();
    let (a, b) = (server(&host, db.path()), server(&host, db.path()));
    for s in [&a, &b] {
        ok(&s.config, &["release", "install", "blog", "2.0.0"]);
    }
    let switches: Vec<_> = [a.config.clone(), b.config.clone()]
        .into_iter()
        .map(|config| {
            std::thread::spawn(move || run(&config, &["release", "switch", "blog@2.0.0"]))
        })
        .collect();
    for switch in switches {
        let (code, out) = switch.join().unwrap();
        assert_eq!(code, 0, "{out}");
    }
    assert_eq!(
        std::fs::read_to_string(db.path().join("count")).unwrap(),
        "1",
        "applied once"
    );
    for s in [&a, &b] {
        assert_eq!(get(s.addr, "/blog/"), (200, "v2.0.0".into()));
    }
}
