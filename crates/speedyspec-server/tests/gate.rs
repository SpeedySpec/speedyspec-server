//! The admin gate end to end: release access rules, the gate app's site state and session answers.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use support::run::{Running, start};
use support::runtime::{Workers, release};

const ALICE: &str = "wordpress_logged_in_x=alice";

pub struct Site {
    pub dir: tempfile::TempDir,
    pub server: Running,
    pub addr: std::net::SocketAddr,
}

impl Site {
    pub fn api_content(&self) -> PathBuf {
        self.dir.path().join("site-admin-api/wp-content")
    }

    pub fn session_calls(&self) -> usize {
        std::fs::read_to_string(self.api_content().join("session-calls"))
            .map(|text| text.lines().count())
            .unwrap_or(0)
    }

    pub fn set_site(&self, installed: bool) {
        std::fs::write(
            self.api_content().join("site.json"),
            format!(r#"{{"installed":{installed},"maintenance":false}}"#),
        )
        .unwrap();
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn admin_site(dir: &Path) -> PathBuf {
    let root = dir.join("release-admin");
    write(
        &root.join("speedyspec-release.json"),
        r#"{"app":"admin","version":"1.0.0","kind":"static","static":{"root":".","access":[
  {"match":"","public":"always"},{"match":"signin","public":"always"},{"match":"signin/**","public":"always"},
  {"match":"install/**","public":"not-installed"},
  {"match":"components/{page}/**","page":"{page}"},{"match":"contracts/{page}/**","page":"{page}"}]}}"#,
    );
    for (file, text) in [
        ("index.html", "shell"),
        ("signin/index.html", "sign in"),
        ("install/index.html", "install"),
        ("kit/frame.js", "frame"),
        ("components/core/general/view.js", "general"),
        ("components/core/users/view.js", "users"),
    ] {
        write(&root.join(file), text);
    }
    root
}

/// The admin API: its gate endpoints read files in its site's wp-content.
fn admin_api(dir: &Path) -> PathBuf {
    let root = dir.join("release-admin-api");
    write(
        &root.join("speedyspec-release.json"),
        r#"{"app":"admin-api","version":"1.0.0","kind":"php","php":{"health":{"live":"/gate/health.php","ready":"/gate/health.php"},"gate":{"version":1,"site":"/gate/site.php","session":"/gate/session.php","changes":"/gate/changes.php"}}}"#,
    );
    write(
        &root.join("index.php"),
        "<?php\nfile_put_contents(WP_CONTENT_DIR . '/visitor-calls', $_SERVER['REQUEST_URI'] . \"\\n\", FILE_APPEND);\necho 'api';\n",
    );
    write(
        &root.join("gate/site.php"),
        "<?php\n$f = WP_CONTENT_DIR . '/site.json';\nheader('Content-Type: application/json');\necho is_file($f) ? file_get_contents($f) : '{\"installed\":true,\"maintenance\":false}';\n",
    );
    write(
        &root.join("gate/session.php"),
        &format!(
            "<?php\nfile_put_contents(WP_CONTENT_DIR . '/session-calls', ($_SERVER['HTTP_COOKIE'] ?? '') . \"\\n\", FILE_APPEND);\n$deny = WP_CONTENT_DIR . '/deny';\nif (strpos($_SERVER['HTTP_COOKIE'] ?? '', '{ALICE}') !== false && !is_file($deny)) {{\n  header('Content-Type: application/json');\n  echo '{{\"user\":{{\"id\":5}},\"pages\":[\"core/general\"]}}';\n}} else {{\n  http_response_code(401);\n}}\n"
        ),
    );
    write(
        &root.join("gate/changes.php"),
        "<?php\n$f = WP_CONTENT_DIR . '/changes.json';\nheader('Content-Type: application/json');\necho is_file($f) ? file_get_contents($f) : json_encode(['cursor' => '0', 'users' => []]);\n",
    );
    write(
        &root.join("gate/health.php"),
        "<?php\nif (is_file(WP_CONTENT_DIR . '/unhealthy')) { http_response_code(500); }\necho 'ok';\n",
    );
    write(
        &root.join("v1/session/signout.php"),
        &format!(
            "<?php\nheader('Set-Cookie: wordpress_logged_in_x=; Max-Age=0; Path=/');\nfile_put_contents(WP_CONTENT_DIR . '/deny', '1');\necho 'bye';\n"
        ),
    );
    root
}

pub fn site() -> Site {
    site_with("")
}

/// As [`site`], with more `[gate]` settings.
pub fn site_with(gate_extra: &str) -> Site {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("site-admin-api/wp-content")).unwrap();
    let toml = format!(
        "mode = \"immutable\"\nrun_dir = \"{run}\"\n\n[[listener]]\naddress = \"127.0.0.1:0\"\n\n[access]\nimplementation = \"gate\"\n\n[gate]\napp = \"admin-api\"\nsite_ttl = \"200ms\"\npoll = \"200ms\"\n{gate_extra}\n[health]\ninterval = \"200ms\"\n\n[app.server]\nkind = \"server\"\n\n[app.admin]\nkind = \"static\"\npath = \"{admin}\"\n\n[app.admin-api]\nkind = \"php\"\npath = \"{api}\"\n[app.admin-api.php]\nsite_dir = \"{site}\"\n\n[app.rt]\nkind = \"runtime\"\npath = \"{rt}\"\n\n[[mount]]\npath = \"/admin/\"\napp = \"admin\"\naccess = \"signed-in\"\n\n[[mount]]\npath = \"/admin-api/\"\napp = \"admin-api\"\naccess = \"signed-in\"\n",
        run = dir.path().join("run").display(),
        admin = admin_site(dir.path()).display(),
        api = admin_api(dir.path()).display(),
        site = dir.path().join("site-admin-api").display(),
        rt = release(&dir.path().join("rt"), "1.0.0", Workers::Real).display(),
    );
    let config = dir.path().join("server.toml");
    std::fs::write(&config, toml).unwrap();
    let mut server = start(&config);
    let addr = server.address();
    Site { dir, server, addr }
}

pub fn get(addr: std::net::SocketAddr, path: &str, cookie: Option<&str>) -> (u16, String) {
    let mut request = reqwest::blocking::Client::new()
        .get(format!("http://{addr}{path}"))
        .timeout(Duration::from_secs(10));
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    let response = request.send().unwrap();
    (
        response.status().as_u16(),
        response.text().unwrap_or_default(),
    )
}

#[test]
fn anonymous_gets_signin_only() {
    let site = site();
    assert_eq!(get(site.addr, "/admin/", None), (200, "shell".into()));
    assert_eq!(get(site.addr, "/admin/signin/", None).0, 200);
    assert_eq!(get(site.addr, "/admin/kit/frame.js", None).0, 401);
    assert_eq!(get(site.addr, "/admin/missing.js", None).0, 401);
}

#[test]
fn signed_in_user_gets_shared_and_own_pages() {
    let site = site();
    assert_eq!(
        get(site.addr, "/admin/kit/frame.js", Some(ALICE)),
        (200, "frame".into())
    );
    assert_eq!(
        get(
            site.addr,
            "/admin/components/core/general/view.js",
            Some(ALICE)
        ),
        (200, "general".into())
    );
    assert_eq!(
        get(
            site.addr,
            "/admin/components/core/users/view.js",
            Some(ALICE)
        )
        .0,
        403
    );
    assert_eq!(
        get(
            site.addr,
            "/admin/components/core/users/missing.js",
            Some(ALICE)
        )
        .0,
        403
    );
    assert_eq!(
        get(site.addr, "/admin/kit/frame.js", Some("other=1")).0,
        401
    );
}

#[test]
fn install_page_follows_site_state() {
    let site = site();
    assert_eq!(get(site.addr, "/admin/install/", None).0, 404);
    site.set_site(false);
    let deadline = Instant::now() + Duration::from_secs(3);
    while get(site.addr, "/admin/install/", None).0 != 200 {
        assert!(Instant::now() < deadline, "the install page never opened");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        get(site.addr, "/admin/install/", None),
        (200, "install".into())
    );
}

#[test]
fn admin_api_paths_are_gated_too() {
    let site = site();
    assert_eq!(get(site.addr, "/admin-api/v1/posts", None).0, 401);
    assert!(
        !site.api_content().join("visitor-calls").exists(),
        "the refused request never reached PHP"
    );
    assert_eq!(
        get(site.addr, "/admin-api/v1/posts", Some(ALICE)),
        (200, "api".into())
    );
}

#[test]
fn session_answers_are_remembered() {
    let site = site();
    assert_eq!(get(site.addr, "/admin/kit/frame.js", Some(ALICE)).0, 200);
    assert_eq!(get(site.addr, "/admin/kit/frame.js", Some(ALICE)).0, 200);
    assert_eq!(
        site.session_calls(),
        1,
        "the second request used the remembered answer"
    );
}

#[test]
fn no_gate_release_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let site = dir.path().join("site");
    std::fs::create_dir_all(&site).unwrap();
    let toml = format!(
        "mode = \"managed\"\ndata_dir = \"{data}\"\nrun_dir = \"{run}\"\n\n[[listener]]\naddress = \"127.0.0.1:0\"\n\n[access]\nimplementation = \"gate\"\n\n[gate]\napp = \"admin-api\"\n\n[app.server]\nkind = \"server\"\n\n[app.admin]\nkind = \"static\"\nsource = \"http://127.0.0.1:9/admin/index.json\"\nallow_unsigned = true\nupdate = \"manual\"\n\n[app.admin-api]\nkind = \"php\"\nsource = \"http://127.0.0.1:9/api/index.json\"\nallow_unsigned = true\nupdate = \"manual\"\n[app.admin-api.php]\nsite_dir = \"{site}\"\n\n[app.rt]\nkind = \"runtime\"\nsource = \"http://127.0.0.1:9/rt/index.json\"\nallow_unsigned = true\nupdate = \"manual\"\n\n[[mount]]\npath = \"/admin/\"\napp = \"admin\"\naccess = \"signed-in\"\n",
        data = dir.path().join("data").display(),
        run = dir.path().join("run").display(),
        site = site.display(),
    );
    let config = dir.path().join("server.toml");
    std::fs::write(&config, toml).unwrap();
    let mut server = start(&config);
    let addr = server.address();
    assert_eq!(get(addr, "/admin/kit/frame.js", Some(ALICE)).0, 401);
    assert_eq!(get(addr, "/admin/", None).0, 401);
}

#[test]
fn signout_takes_effect_at_once() {
    let site = site();
    assert_eq!(get(site.addr, "/admin/kit/frame.js", Some(ALICE)).0, 200);
    let calls = site.session_calls();
    let (status, body) = get(site.addr, "/admin-api/v1/session/signout.php", Some(ALICE));
    assert_eq!((status, body.as_str()), (200, "bye"));
    // The very next request with the old cookie is asked about again, and refused.
    assert_eq!(get(site.addr, "/admin/kit/frame.js", Some(ALICE)).0, 401);
    // The sign-out request itself used the remembered answer; the refusal came from a fresh check (without the
    // sign-out forgetting it, the answer would be remembered for session_ttl, 60 s).
    assert_eq!(
        site.session_calls(),
        calls + 1,
        "exactly one fresh check, after the sign-out"
    );
}

#[test]
fn a_change_takes_effect_within_a_second() {
    let site = site();
    assert_eq!(get(site.addr, "/admin/kit/frame.js", Some(ALICE)).0, 200);
    // The gate app now refuses alice, and its feed names her: the remembered answer must not outlive the next poll.
    std::fs::write(site.api_content().join("deny"), "1").unwrap();
    std::fs::write(
        site.api_content().join("changes.json"),
        r#"{"cursor":"7","users":[5]}"#,
    )
    .unwrap();
    let started = Instant::now();
    while get(site.addr, "/admin/kit/frame.js", Some(ALICE)).0 != 401 {
        assert!(
            started.elapsed() < Duration::from_millis(1500),
            "still allowed after {:?}",
            started.elapsed()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn changes_path_is_not_reachable_by_visitors() {
    let site = site();
    assert_eq!(
        get(site.addr, "/admin-api/gate/changes.php", Some(ALICE)).0,
        404
    );
    assert_eq!(get(site.addr, "/admin-api/gate/changes.php", None).0, 404);
}

fn wait(addr: std::net::SocketAddr, path: &str, status: u16) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while get(addr, path, Some(ALICE)).0 != status {
        assert!(Instant::now() < deadline, "{path} never answered {status}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn admin_api_site_state_turns_maintenance_on() {
    let site = site();
    assert_eq!(get(site.addr, "/admin/kit/frame.js", Some(ALICE)).0, 200);
    std::fs::write(
        site.api_content().join("site.json"),
        r#"{"installed":true,"maintenance":true}"#,
    )
    .unwrap();
    wait(site.addr, "/admin/kit/frame.js", 503);
    wait(site.addr, "/admin-api/v1/posts", 503);
    std::fs::write(
        site.api_content().join("site.json"),
        r#"{"installed":true,"maintenance":false}"#,
    )
    .unwrap();
    wait(site.addr, "/admin/kit/frame.js", 200);
}

#[test]
fn maintenance_apps_limits_it() {
    let site = site_with("maintenance_apps = [\"admin\"]\n");
    std::fs::write(
        site.api_content().join("site.json"),
        r#"{"installed":true,"maintenance":true}"#,
    )
    .unwrap();
    wait(site.addr, "/admin/kit/frame.js", 503);
    assert_eq!(
        get(site.addr, "/admin-api/v1/posts", Some(ALICE)),
        (200, "api".into()),
        "not a maintenance app"
    );
}

#[test]
fn gate_app_health_covers_the_admin() {
    let site = site();
    assert_eq!(get(site.addr, "/admin/kit/frame.js", Some(ALICE)).0, 200);
    std::fs::write(site.api_content().join("unhealthy"), "1").unwrap();
    wait(site.addr, "/admin/kit/frame.js", 503);
    assert_eq!(
        get(site.addr, "/admin/", None).0,
        503,
        "the whole admin, public entries too"
    );
}
