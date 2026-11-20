//! Per-app maintenance from failing health probes: only that app, its page from memory, its probes not reachable.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use support::run::{Running, start};
use support::runtime::{Workers, release};

const PAGE: &str = "<!doctype html><title>Down for maintenance</title><p>Back soon.</p>";

struct Site {
    dir: tempfile::TempDir,
    server: Running,
    addr: std::net::SocketAddr,
}

impl Site {
    fn site_dir(&self, app: &str) -> PathBuf {
        self.dir.path().join(format!("site-{app}"))
    }

    /// Makes `app`'s probes answer `status` (200 removes the override).
    fn health(&self, app: &str, status: u16) {
        let flag = self.site_dir(app).join("wp-content/health");
        if status == 200 {
            let _ = std::fs::remove_file(flag);
        } else {
            std::fs::write(flag, status.to_string()).unwrap();
        }
    }
}

fn php_release(dir: &Path, app: &str) -> PathBuf {
    let root = dir.join(format!("release-{app}"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("speedyspec-release.json"),
        format!(
            r#"{{"app":"{app}","version":"1.0.0","kind":"php","php":{{"health":{{"live":"/health.php","ready":"/health.php"}}}}}}"#
        ),
    )
    .unwrap();
    std::fs::write(root.join("index.php"), format!("<?php echo '{app} ok';\n")).unwrap();
    std::fs::write(
        root.join("sleep.php"),
        "<?php sleep((int) ($_GET['s'] ?? 1)); echo 'slept';\n",
    )
    .unwrap();
    std::fs::write(
        root.join("health.php"),
        "<?php\nfile_put_contents(WP_CONTENT_DIR . '/calls', ($_SERVER['REMOTE_ADDR'] ?? '') . \"\\n\", FILE_APPEND);\n$flag = WP_CONTENT_DIR . '/health';\nif (is_file($flag)) { http_response_code((int) file_get_contents($flag)); }\necho 'ok';\n",
    )
    .unwrap();
    root
}

fn site() -> Site {
    let dir = tempfile::tempdir().unwrap();
    let admin = dir.path().join("release-admin");
    std::fs::create_dir_all(&admin).unwrap();
    std::fs::write(
        admin.join("speedyspec-release.json"),
        r#"{"app":"admin","version":"1.0.0","kind":"static","static":{"root":".","maintenance_page":"maintenance.html"}}"#,
    )
    .unwrap();
    std::fs::write(admin.join("index.html"), "admin").unwrap();
    std::fs::write(admin.join("maintenance.html"), PAGE).unwrap();
    let mut apps = String::new();
    for app in ["blog", "frontend-api"] {
        let site = dir.path().join(format!("site-{app}"));
        std::fs::create_dir_all(site.join("wp-content")).unwrap();
        apps.push_str(&format!(
            "[app.{app}]\nkind = \"php\"\npath = \"{}\"\nmaintenance_page = \"admin\"\n[app.{app}.php]\nsite_dir = \"{}\"\nworkers = {{ min = 1, max = 1 }}\n\n",
            php_release(dir.path(), app).display(),
            site.display()
        ));
    }
    let toml = format!(
        "mode = \"immutable\"\nrun_dir = \"{run}\"\n\n[[listener]]\naddress = \"127.0.0.1:0\"\n\n[health]\ninterval = \"200ms\"\ntimeout = \"1s\"\nfail_after = 3\npass_after = 3\n\n[app.server]\nkind = \"server\"\n\n[app.admin]\nkind = \"static\"\npath = \"{admin}\"\n\n[app.rt]\nkind = \"runtime\"\npath = \"{rt}\"\n\n{apps}[[mount]]\npath = \"/blog/\"\napp = \"blog\"\n\n[[mount]]\npath = \"/api/\"\napp = \"frontend-api\"\n\n[[mount]]\npath = \"/admin/\"\napp = \"admin\"\n",
        run = dir.path().join("run").display(),
        admin = admin.display(),
        rt = release(&dir.path().join("rt"), "1.0.0", Workers::Real).display(),
    );
    let config = dir.path().join("server.toml");
    std::fs::write(&config, toml).unwrap();
    let mut server = start(&config);
    let addr = server.address();
    Site { dir, server, addr }
}

fn get(addr: std::net::SocketAddr, path: &str) -> reqwest::blocking::Response {
    reqwest::blocking::Client::new()
        .get(format!("http://{addr}{path}"))
        .timeout(Duration::from_secs(10))
        .send()
        .unwrap()
}

fn wait_status(addr: std::net::SocketAddr, path: &str, status: u16) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if get(addr, path).status() == status {
            return;
        }
        assert!(Instant::now() < deadline, "{path} never answered {status}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// What an unhealthy app answers: a plain server error, not the maintenance page. Failing health is an outage, not
/// planned maintenance.
const UNAVAILABLE: &str = "service unavailable";

#[test]
fn failing_health_answers_only_that_app_with_a_server_error() {
    let site = site();
    wait_status(site.addr, "/blog/", 200);
    site.health("blog", 500);
    wait_status(site.addr, "/blog/", 503);
    let response = get(site.addr, "/blog/anything");
    assert_eq!(response.status(), 503);
    assert!(
        response.headers().get("retry-after").is_none(),
        "an outage has no expected end"
    );
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert!(
        response.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/plain")
    );
    assert_eq!(response.text().unwrap(), UNAVAILABLE);
    let api = get(site.addr, "/api/");
    assert_eq!(
        (api.status().as_u16(), api.text().unwrap()),
        (200, "frontend-api ok".to_string())
    );
    assert!(
        site.server.has_event("app_unhealthy"),
        "{:?}",
        site.server.log()
    );
}

#[test]
fn recovery_ends_maintenance() {
    let site = site();
    site.health("blog", 500);
    wait_status(site.addr, "/blog/", 503);
    site.health("blog", 200);
    wait_status(site.addr, "/blog/", 200);
}

#[test]
fn unhealthy_answer_needs_no_php() {
    let site = site();
    wait_status(site.addr, "/blog/", 200);
    let addr = site.addr;
    // The blog's only visitor worker is busy for a while.
    let busy = std::thread::spawn(move || get(addr, "/blog/sleep.php?s=4").status());
    std::thread::sleep(Duration::from_millis(300));
    site.health("blog", 500);
    wait_status(site.addr, "/blog/", 503);
    let started = Instant::now();
    assert_eq!(get(site.addr, "/blog/").text().unwrap(), UNAVAILABLE);
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "answered without PHP"
    );
    let _ = busy.join();
}

#[test]
fn health_paths_are_not_reachable_by_visitors() {
    let site = site();
    wait_status(site.addr, "/blog/", 200);
    for path in ["/blog/health.php", "/blog/health.php/", "/blog//health.php"] {
        assert_eq!(get(site.addr, path).status(), 404, "{path}");
    }
    let calls = std::fs::read_to_string(site.site_dir("blog").join("wp-content/calls")).unwrap();
    assert!(!calls.is_empty(), "the server probed the app");
    assert!(
        calls.lines().all(str::is_empty),
        "only the server's own calls (no client address): {calls:?}"
    );
}
