//! The server's cron scheduler end to end: the real server calls a PHP app's declared cron path when the app's
//! workers report it is due, on its own internal worker, and visitors cannot reach it.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use support::run::{Running, start};
use support::runtime::{Workers, release};

struct Site {
    dir: tempfile::TempDir,
    _server: Running,
    addr: std::net::SocketAddr,
}

impl Site {
    fn content(&self) -> PathBuf {
        self.dir.path().join("site/wp-content")
    }

    /// When each cron call happened (seconds, as PHP's `microtime( true )`), and who made it.
    fn calls(&self) -> Vec<(f64, String)> {
        std::fs::read_to_string(self.content().join("calls.txt"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let (at, who) = line.split_once(' ').unwrap_or((line, ""));
                Some((at.parse().ok()?, who.to_string()))
            })
            .collect()
    }

    fn wait_calls(&self, n: usize, within: Duration) -> Vec<(f64, String)> {
        let deadline = Instant::now() + within;
        loop {
            let calls = self.calls();
            if calls.len() >= n || Instant::now() > deadline {
                return calls;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// A php release declaring `php.cron` and `/wp-cron.php` as internal. `cron.php` appends the time and the client
/// address to `calls.txt`, sleeps as long as `sleep` says, and reports its next time `next` seconds ahead (default 2).
fn php_release(dir: &Path) -> PathBuf {
    let root = dir.join("release-app");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("speedyspec-release.json"),
        r#"{"app":"admin-api","version":"1.0.0","kind":"php","php":{"health":{"live":"/health.php","ready":"/health.php"},"cron":{"path":"/cron.php"},"internal":["/wp-cron.php"]}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("index.php"),
        "<?php if (is_file(WP_CONTENT_DIR . '/wake')) { \\SpeedySpec\\Runtime\\report_next_wake(time()); } echo 'app ok';\n",
    )
    .unwrap();
    std::fs::write(root.join("health.php"), "<?php echo 'ok';\n").unwrap();
    std::fs::write(root.join("wp-cron.php"), "<?php echo 'wp-cron';\n").unwrap();
    std::fs::write(
        root.join("cron.php"),
        "<?php\n$c = WP_CONTENT_DIR;\nfile_put_contents(\"$c/calls.txt\", sprintf(\"%.3F %s\\n\", microtime(true), $_SERVER['REMOTE_ADDR'] ?? ''), FILE_APPEND);\nif (is_file(\"$c/sleep\")) { sleep((int) file_get_contents(\"$c/sleep\")); }\n$next = is_file(\"$c/next\") ? (int) file_get_contents(\"$c/next\") : 2;\n\\SpeedySpec\\Runtime\\report_next_wake(time() + $next);\necho '{}';\n",
    )
    .unwrap();
    root
}

/// A server with that app at `/api/`, `max_interval = "10s"`, and `files` written into its `wp-content` first.
fn site(files: &[(&str, &str)]) -> Site {
    let dir = tempfile::tempdir().unwrap();
    let content = dir.path().join("site/wp-content");
    std::fs::create_dir_all(&content).unwrap();
    for (name, body) in files {
        std::fs::write(content.join(name), body).unwrap();
    }
    let toml = format!(
        "mode = \"immutable\"\nrun_dir = \"{run}\"\n\n[[listener]]\naddress = \"127.0.0.1:0\"\n\n[health]\ninterval = \"200ms\"\ntimeout = \"1s\"\nfail_after = 3\npass_after = 3\n\n[app.server]\nkind = \"server\"\n\n[app.rt]\nkind = \"runtime\"\npath = \"{rt}\"\n\n[app.admin-api]\nkind = \"php\"\npath = \"{app}\"\n[app.admin-api.php]\nsite_dir = \"{site}\"\nworkers = {{ min = 1, max = 1 }}\n[app.admin-api.php.cron]\nmax_interval = \"10s\"\n\n[[mount]]\npath = \"/api/\"\napp = \"admin-api\"\n",
        run = dir.path().join("run").display(),
        rt = release(&dir.path().join("rt"), "1.0.0", Workers::Real).display(),
        app = php_release(dir.path()).display(),
        site = dir.path().join("site").display(),
    );
    let config = dir.path().join("server.toml");
    std::fs::write(&config, toml).unwrap();
    let mut server = start(&config);
    let addr = server.address();
    Site {
        dir,
        _server: server,
        addr,
    }
}

fn get(addr: std::net::SocketAddr, path: &str) -> reqwest::blocking::Response {
    reqwest::blocking::Client::new()
        .get(format!("http://{addr}{path}"))
        .timeout(Duration::from_secs(10))
        .send()
        .unwrap()
}

#[test]
fn cron_is_called_at_the_reported_time() {
    let site = site(&[]);
    let calls = site.wait_calls(3, Duration::from_secs(9));
    assert!(calls.len() >= 3, "{calls:?}");
    for pair in calls.windows(2) {
        let gap = pair[1].0 - pair[0].0;
        // Each call reports its next time two seconds ahead; the interval without reports is ten.
        assert!((1.0..5.0).contains(&gap), "{calls:?}");
    }
}

#[test]
fn cron_and_internal_paths_404_for_visitors() {
    let site = site(&[]);
    site.wait_calls(1, Duration::from_secs(5));
    for path in [
        "/api/cron.php",
        "/api/wp-cron.php",
        "/api/cron.php?budget=1",
    ] {
        assert_eq!(get(site.addr, path).status(), 404, "{path}");
    }
    assert_eq!(get(site.addr, "/api/").status(), 200);
    let calls = site.calls();
    assert!(!calls.is_empty());
    assert!(
        calls.iter().all(|(_, who)| who.is_empty()),
        "only the server's own calls: {calls:?}"
    );
}

#[test]
fn long_cron_does_not_block_probes() {
    let site = site(&[("sleep", "5")]);
    site.wait_calls(1, Duration::from_secs(5));
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(1200));
        // Health probes run on their own worker: three failed probes (600 ms) would answer 503 here.
        assert_eq!(get(site.addr, "/api/").status(), 200);
    }
}

#[test]
fn visitor_report_pulls_cron_forward() {
    // Each call reports its next time a minute ahead, so the scheduler would wait its full ten seconds.
    let site = site(&[("next", "60")]);
    let first = site.wait_calls(1, Duration::from_secs(5));
    assert_eq!(first.len(), 1, "{first:?}");
    std::fs::write(site.content().join("wake"), "").unwrap();
    let asked = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    assert_eq!(get(site.addr, "/api/").status(), 200);
    let calls = site.wait_calls(2, Duration::from_secs(4));
    assert!(calls.len() >= 2, "{calls:?}");
    assert!(
        calls[1].0 - asked < 3.0,
        "the visitor's report pulled the call forward: {calls:?}"
    );
}
