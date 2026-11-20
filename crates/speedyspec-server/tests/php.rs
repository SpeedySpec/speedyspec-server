//! PHP apps end to end: the real binary, a pool of `speedyspec-php-runtime worker` processes, real sockets, and the
//! scripts in tests/fixtures/php-app.

mod support;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use support::run::{Running, start};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A temp directory with a site directory and an immutable config serving the php-app fixture at `/`.
struct Setup {
    dir: tempfile::TempDir,
    config: PathBuf,
}

impl Setup {
    fn site(&self) -> PathBuf {
        self.dir.path().join("site")
    }
}

fn setup(php: &str) -> Setup {
    support::runtime::binary();
    let dir = tempfile::tempdir().unwrap();
    let site = dir.path().join("site");
    std::fs::create_dir_all(site.join("uploads")).unwrap();
    std::fs::create_dir_all(site.join("wp-content")).unwrap();
    std::fs::create_dir_all(site.join("wp-content/uploads")).unwrap();
    std::fs::create_dir_all(site.join("wp-content/plugins/x")).unwrap();
    std::fs::write(site.join("wp-content/uploads/a.txt"), "uploaded\n").unwrap();
    std::fs::write(site.join("wp-content/shared.txt"), "from the site\n").unwrap();
    std::fs::write(site.join("uploads/private.txt"), "private\n").unwrap();
    std::fs::write(
        site.join("wp-content/plugins/x/endpoint.php"),
        "<?php\necho 'site endpoint';\n",
    )
    .unwrap();
    std::fs::write(
        site.join(".env"),
        "# a comment\nSPEEDYSPEC_TEST_ENV=from-dotenv\n",
    )
    .unwrap();
    let toml = format!(
        r#"mode = "immutable"
run_dir = "{run}"

[[listener]]
address = "127.0.0.1:0"

[app.server]
kind = "server"

[app.blog]
kind = "php"
path = "{app}"

[app.blog.php]
site_dir = "{site}"
{php}

[app.rt]
kind = "runtime"
path = "{runtime}"

[[mount]]
path = "/"
app = "blog"
"#,
        runtime = support::runtime::release(
            &dir.path().join("rt"),
            "1.0.0",
            support::runtime::Workers::Real
        )
        .display(),
        run = dir.path().join("run").display(),
        app = fixture("php-app").display(),
        site = site.display(),
    );
    let config = dir.path().join("server.toml");
    std::fs::write(&config, toml).unwrap();
    Setup { dir, config }
}

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap()
}

fn get(addr: SocketAddr, path: &str) -> (u16, String) {
    let response = client().get(format!("http://{addr}{path}")).send().unwrap();
    let status = response.status().as_u16();
    (status, response.text().unwrap_or_default())
}

fn serve(php: &str) -> (Setup, Running, SocketAddr) {
    let setup = setup(php);
    let mut server = start(&setup.config);
    let addr = server.address();
    (setup, server, addr)
}

fn count_events(server: &Running, event: &str, field: Option<(&str, &str)>) -> usize {
    server
        .log()
        .iter()
        .filter(|line| line.contains(&format!("\"{event}\"")))
        .filter(|line| field.is_none_or(|(k, v)| line.contains(k) && line.contains(v)))
        .count()
}

#[test]
fn server_vars_and_body_round_trip() {
    let (_setup, server, addr) = serve("");
    let response = client()
        .post(format!("http://{addr}/server.php?a=1&b=two"))
        .header("X-Thing", "y")
        .header("Cookie", "c=cookie")
        .header("Content-Type", "text/plain")
        .body("hello body")
        .send()
        .unwrap();
    assert_eq!(response.status(), 200, "{:?}", server.log());
    assert_eq!(response.headers()["content-type"], "application/json");
    let json: serde_json::Value = serde_json::from_str(&response.text().unwrap()).unwrap();
    let s = &json["server"];
    assert_eq!(s["REQUEST_METHOD"], "POST");
    assert_eq!(s["QUERY_STRING"], "a=1&b=two");
    assert_eq!(s["REQUEST_URI"], "/server.php?a=1&b=two");
    assert_eq!(s["SCRIPT_NAME"], "/server.php");
    assert_eq!(s["HTTP_X_THING"], "y");
    assert_eq!(s["REMOTE_ADDR"], "127.0.0.1");
    assert_eq!(s["CONTENT_TYPE"], "text/plain");
    assert_eq!(s["CONTENT_LENGTH"], "10");
    assert_eq!(s["GATEWAY_INTERFACE"], "CGI/1.1");
    assert_eq!(s["SERVER_PORT"], addr.port().to_string());
    assert_eq!(json["get"]["b"], "two");
    assert_eq!(json["body"], "hello body");
    assert_eq!(json["cookie"]["c"], "cookie");
    assert_eq!(json["memory_limit"], "512M", "the release's ini applies");
    assert_eq!(
        json["upload_max_filesize"], "64M",
        "per-directory settings apply too"
    );
    assert_eq!(
        json["content_url"],
        serde_json::Value::Null,
        "WP_CONTENT_URL is never built from Host"
    );
    assert!(
        json["content_dir"]
            .as_str()
            .unwrap()
            .ends_with("site/wp-content")
    );
    assert_eq!(json["env"], "from-dotenv", "site_dir/.env reaches getenv()");
}

#[test]
fn front_controller_gets_unmatched_paths() {
    let (_setup, _server, addr) = serve("");
    assert_eq!(
        get(addr, "/some/pretty/url?x=1"),
        (200, "front:/some/pretty/url?x=1|/index.php|".into())
    );
    assert_eq!(get(addr, "/"), (200, "front:/|/index.php|".into()));
    assert_eq!(get(addr, "/dir/"), (200, "dir index".into()));
}

#[test]
fn php_file_runs_directly() {
    let (_setup, _server, addr) = serve("");
    let (status, body) = get(addr, "/server.php/extra/info");
    assert_eq!(status, 200);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["server"]["SCRIPT_NAME"], "/server.php");
    assert_eq!(json["server"]["PATH_INFO"], "/extra/info");
    assert!(
        json["server"]["SCRIPT_FILENAME"]
            .as_str()
            .unwrap()
            .ends_with("public/server.php")
    );
}

#[test]
fn static_file_from_site_dir_then_release() {
    let (_setup, _server, addr) = serve("");
    assert_eq!(
        get(addr, "/wp-content/shared.txt"),
        (200, "from the site\n".into()),
        "site_dir's wp-content wins"
    );
    assert_eq!(
        get(addr, "/wp-content/uploads/a.txt"),
        (200, "uploaded\n".into())
    );
    let response = client()
        .get(format!("http://{addr}/style.css"))
        .send()
        .unwrap();
    assert_eq!(
        response.headers()["content-type"],
        "text/css; charset=utf-8"
    );
    assert_eq!(response.text().unwrap(), "body { color: red; }\n");
}

/// Only site_dir's wp-content is served from it, and no dotfile anywhere: site_dir holds secrets.
#[test]
fn private_files_are_not_served() {
    let (_setup, _server, addr) = serve("");
    for path in [
        "/.env",
        "/uploads/private.txt",
        "/.env.php",
        "/wp-content/../.env",
    ] {
        let (_, body) = get(addr, path);
        assert!(
            body.starts_with("front:") && !body.contains("SPEEDYSPEC_TEST_ENV="),
            "{path} goes to the front controller, not the file: {body}"
        );
    }
}

/// A directory without its trailing slash redirects, as Apache and nginx do (relative links in it must resolve).
#[test]
fn directory_without_slash_redirects() {
    let (_setup, _server, addr) = serve("");
    let response = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
        .get(format!("http://{addr}/dir?x=1"))
        .send()
        .unwrap();
    assert_eq!(response.status(), 301);
    assert_eq!(response.headers()["location"], "/dir/?x=1");
}

#[test]
fn php_in_site_wp_content_runs() {
    let (_setup, _server, addr) = serve("");
    assert_eq!(
        get(addr, "/wp-content/plugins/x/endpoint.php"),
        (200, "site endpoint".into())
    );
}

/// A client that goes away does not kill the script: WP-Cron relies on ignore_user_abort().
#[test]
fn client_disconnect_lets_script_finish() {
    let (setup, _server, addr) = serve("");
    let finished = setup.site().join("wp-content/finished");
    // The first request after start pays one-time costs; this test is about a client leaving mid-script.
    assert_eq!(get(addr, "/index.php").0, 200);
    {
        let mut stream = TcpStream::connect(addr).unwrap();
        write!(
            stream,
            "GET /finish.php HTTP/1.1\r\nHost: localhost\r\n\r\n"
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(100));
    }
    let deadline = Instant::now() + Duration::from_secs(4);
    while !finished.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(finished.exists(), "the script ran to its end");
}

/// The CLI-era `SpeedySpec\Hooks\Hook` registry holds nothing past a request.
#[test]
fn legacy_hook_registry_is_request_scoped() {
    let (_setup, _server, addr) = serve("workers = { min = 1, max = 1 }");
    assert_eq!(get(addr, "/oldhook.php"), (200, "clean".into()));
    assert_eq!(get(addr, "/oldhook.php"), (200, "clean".into()));
}

/// Reads a raw HTTP/1.1 response, calling `chunk` with the time each read arrives.
fn raw_get(addr: SocketAddr, path: &str, mut chunk: impl FnMut(&[u8], Duration)) {
    let mut stream = TcpStream::connect(addr).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let started = Instant::now();
    let mut buf = [0u8; 8192];
    loop {
        let n = stream.read(&mut buf).unwrap();
        if n == 0 {
            return;
        }
        chunk(&buf[..n], started.elapsed());
    }
}

#[test]
fn response_streams() {
    let (_setup, _server, addr) = serve("");
    let mut first = None;
    let mut all = Vec::new();
    raw_get(addr, "/stream.php", |bytes, at| {
        if first.is_none() && String::from_utf8_lossy(bytes).contains("chunk0") {
            first = Some(at);
        }
        all.extend_from_slice(bytes);
    });
    let first = first.expect("chunk0 arrives");
    assert!(
        first < Duration::from_millis(900),
        "the first chunk arrives before the script ends ({first:?})"
    );
    let text = String::from_utf8_lossy(&all);
    assert!(text.contains("chunk4"), "{text}");
}

#[test]
fn fatal_error_is_500_and_next_request_clean() {
    let (_setup, server, addr) = serve("workers = { min = 1, max = 1 }");
    assert_eq!(get(addr, "/fatal.php").0, 500);
    assert_eq!(
        get(addr, "/hooks.php"),
        (200, "clean|v-x".into()),
        "nothing survives the fatal request: {:?}",
        server.log()
    );
    assert_eq!(get(addr, "/throw.php").0, 500);
    assert_eq!(get(addr, "/exit.php"), (200, "before".into()));
    assert!(
        server
            .log()
            .iter()
            .any(|line| line.contains("undefined_function_in_the_fixture")),
        "PHP's message reaches the log: {:?}",
        server.log()
    );
}

#[test]
fn hooks_work_in_a_request() {
    let (_setup, _server, addr) = serve("workers = { min = 1, max = 1 }");
    assert_eq!(get(addr, "/hooks.php"), (200, "clean|v-x".into()));
    assert_eq!(
        get(addr, "/hooks.php"),
        (200, "clean|v-x".into()),
        "the callback is gone with its request"
    );
}

#[test]
fn crash_is_502_and_worker_replaced() {
    let (_setup, server, addr) = serve("workers = { min = 1, max = 1 }");
    let (_, before) = get(addr, "/pid.php");
    assert_eq!(get(addr, "/crash.php").0, 502);
    let (status, after) = get(addr, "/pid.php");
    assert_eq!(status, 200, "a replacement answers: {:?}", server.log());
    assert_ne!(before, after);
    assert!(
        count_events(&server, "php_worker_exited", None) >= 1,
        "{:?}",
        server.log()
    );
}

#[test]
fn hang_is_504_and_worker_killed() {
    let (_setup, server, addr) = serve("workers = { min = 1, max = 1 }\nrequest_timeout = \"1s\"");
    let started = Instant::now();
    assert_eq!(get(addr, "/sleep.php?s=10").0, 504);
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    assert!(
        server.wait_for_event("php_request_timeout", Duration::from_secs(2)),
        "{:?}",
        server.log()
    );
    assert_eq!(
        get(addr, "/sleep.php?s=0"),
        (200, "slept".into()),
        "a replacement answers"
    );
}

#[test]
fn slow_upload_bounded_by_request_timeout() {
    let (_setup, server, addr) = serve("workers = { min = 1, max = 1 }\nrequest_timeout = \"1s\"");
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "POST /upload.php HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/octet-stream\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\n0123456789"
    )
    .unwrap();
    let started = Instant::now();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    let text = String::from_utf8_lossy(&response);
    assert!(
        text.starts_with("HTTP/1.1 504"),
        "{text:?}; log: {:?}",
        server.log()
    );
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(get(addr, "/upload.php").0, 200, "the worker is free again");
}

#[test]
fn slow_reader_gets_streamed_response() {
    let (setup, _server, addr) = serve("");
    let progress = setup.site().join("wp-content/progress");
    let mut stream = TcpStream::connect(addr).unwrap();
    write!(
        stream,
        "GET /big.php HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut buf = vec![0u8; 65536];
    let mut total = stream.read(&mut buf).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let written: u32 = std::fs::read_to_string(&progress)
        .unwrap_or_default()
        .trim()
        .parse()
        .unwrap_or(0);
    assert!(
        written < 200,
        "the script waits for the slow reader ({written} of 200 chunks written)"
    );
    loop {
        let n = stream.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        total += n;
    }
    assert!(total > 200 * 65536, "every byte arrives ({total})");
}

#[test]
fn leak_recycles_worker() {
    let (_setup, server, addr) = serve("workers = { min = 1, max = 1 }\nmemory_limit = \"16MiB\"");
    let (_, before) = get(addr, "/pid.php");
    assert_eq!(
        get(addr, "/leak.php"),
        (200, (40 * 1024 * 1024).to_string())
    );
    let (_, after) = get(addr, "/pid.php");
    assert_ne!(before, after, "{:?}", server.log());
    assert_eq!(
        count_events(
            &server,
            "php_worker_recycled",
            Some(("reason", "memory_limit"))
        ),
        1,
        "{:?}",
        server.log()
    );
}

#[test]
fn max_requests_recycles_worker() {
    let (_setup, server, addr) = serve("workers = { min = 1, max = 1 }\nmax_requests = 2");
    let (_, one) = get(addr, "/pid.php");
    let (_, two) = get(addr, "/pid.php");
    let (_, three) = get(addr, "/pid.php");
    assert_eq!(one, two);
    assert_ne!(two, three, "{:?}", server.log());
    assert!(
        count_events(
            &server,
            "php_worker_recycled",
            Some(("reason", "max_requests"))
        ) >= 1
    );
}

#[test]
fn grows_to_max_and_shrinks_after_idle() {
    let (_setup, server, addr) = serve("workers = { min = 1, max = 3 }\nidle_timeout = \"1s\"");
    let started = Instant::now();
    let requests: Vec<_> = (0..3)
        .map(|_| std::thread::spawn(move || get(addr, "/sleep.php?s=2")))
        .collect();
    for request in requests {
        assert_eq!(request.join().unwrap(), (200, "slept".into()));
    }
    assert!(
        started.elapsed() < Duration::from_millis(4500),
        "three workers ran at once: {:?}",
        started.elapsed()
    );
    // Three visitor workers, and the two internal ones kept for the server's own calls (probes, gate calls).
    assert_eq!(
        count_events(&server, "php_worker_started", None),
        5,
        "{:?}",
        server.log()
    );
    let deadline = Instant::now() + Duration::from_secs(6);
    while count_events(
        &server,
        "php_worker_recycled",
        Some(("reason", "idle_timeout")),
    ) < 2
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        count_events(
            &server,
            "php_worker_recycled",
            Some(("reason", "idle_timeout"))
        ),
        2,
        "{:?}",
        server.log()
    );
    assert_eq!(get(addr, "/pid.php").0, 200, "min stays");
}

#[test]
fn release_switch_finishes_long_request_on_old_pool() {
    use support::release_host::ReleaseHost;
    use support::run::command;

    let host = ReleaseHost::start();
    let manifest = |version: &str| {
        format!(r#"{{"app":"blog","version":"{version}","kind":"php","php":{{}}}}"#)
    };
    let slow = "<?php\nsleep(3);\necho file_get_contents(__DIR__ . '/version.txt');\n";
    let index = "<?php\necho file_get_contents(__DIR__ . '/version.txt');\n";
    host.publish_release(
        "blog",
        "1.0.0",
        &manifest("1.0.0"),
        &[
            ("index.php", index),
            ("slow.php", slow),
            ("version.txt", "v1"),
        ],
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("site")).unwrap();
    let toml = format!(
        r#"mode = "managed"
data_dir = "{data}"
run_dir = "{run}"

[[listener]]
address = "127.0.0.1:0"

[app.server]
kind = "server"

[app.blog]
kind = "php"
source = "{source}"
keys = ["{key}"]
update = "manual"

[app.blog.php]
site_dir = "{site}"

[app.rt]
kind = "runtime"
source = "{runtime_source}"
keys = ["{key}"]
update = "manual"

[[mount]]
path = "/"
app = "blog"
"#,
        runtime_source = host.url("/rt/index.json"),
        data = dir.path().join("data").display(),
        run = dir.path().join("run").display(),
        source = host.url("/blog/index.json"),
        key = host.public_key(),
        site = dir.path().join("site").display(),
    );
    let config = dir.path().join("server.toml");
    std::fs::write(&config, toml).unwrap();
    let config_arg = config.display().to_string();
    let mut server = start(&config);
    let addr = server.address();
    let keeper = dir.path().join("run/keeper.sock");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !keeper.exists() {
        assert!(
            Instant::now() < deadline,
            "no control socket at {}",
            keeper.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    host.publish_binary(
        "rt",
        "1.0.0",
        support::runtime::script("1.0.0", support::runtime::Workers::Real).as_bytes(),
    );
    for args in [
        ["release", "install", "rt", "1.0.0"],
        ["release", "switch", "rt@1.0.0", ""],
    ] {
        let mut all: Vec<&str> = args.iter().copied().filter(|a| !a.is_empty()).collect();
        all.extend_from_slice(&["--config", &config_arg]);
        let (code, out, err) = command(&all);
        assert_eq!(code, 0, "{all:?}: {out}{err}");
    }
    let (code, out, err) = command(&[
        "release",
        "install",
        "blog",
        "1.0.0",
        "--config",
        &config_arg,
    ]);
    assert_eq!(code, 0, "{out}{err}");
    let (code, out, err) = command(&["release", "switch", "blog@1.0.0", "--config", &config_arg]);
    assert_eq!(code, 0, "{out}{err}");
    // The PHP response is chunked, which the support poll does not decode.
    let deadline = Instant::now() + Duration::from_secs(20);
    let (status, body) = loop {
        let answer = client()
            .get(format!("http://{addr}/"))
            .send()
            .map(|r| (r.status().as_u16(), r.text().unwrap_or_default()));
        match answer {
            Ok((200, body)) => break (200, body),
            _ if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            other => panic!("the release never served: {other:?}"),
        }
    };
    assert_eq!((status, body.as_str()), (200, "v1"), "{:?}", server.log());

    let long = std::thread::spawn(move || get(addr, "/slow.php"));
    std::thread::sleep(Duration::from_millis(500));
    host.publish_release(
        "blog",
        "2.0.0",
        &manifest("2.0.0"),
        &[
            ("index.php", index),
            ("slow.php", slow),
            ("version.txt", "v2"),
        ],
    );
    let (code, out, err) = command(&[
        "release",
        "install",
        "blog",
        "2.0.0",
        "--config",
        &config_arg,
    ]);
    assert_eq!(code, 0, "{out}{err}");
    let (code, out, err) = command(&["release", "switch", "blog@2.0.0", "--config", &config_arg]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(
        get(addr, "/"),
        (200, "v2".into()),
        "new requests get the new release"
    );
    assert_eq!(
        long.join().unwrap(),
        (200, "v1".into()),
        "the long request finishes on its own release"
    );
    assert!(
        server.wait_for_event("php_pool_drained", Duration::from_secs(5)),
        "{:?}",
        server.log()
    );
}

/// A worker's spec (the app's credentials) never shows in its argv, and the server's own environment does not reach
/// PHP: only the allowlist and the app's own environment.
#[test]
fn worker_secrets_stay_out_of_argv_and_the_server_environment_stays_out() {
    let setup = setup("");
    let mut server = support::run::start_with(&setup.config, &[("SPEEDYSPEC_SERVER_ONLY", "1")]);
    let addr = server.address();
    assert_eq!(get(addr, "/env.php"), (200, "absent from-dotenv".into()));
    let (_, pid) = get(addr, "/pid.php");
    // Linux has the argv in /proc (its busybox ps cannot print one process's command); macOS has ps.
    let argv = match std::fs::read(format!("/proc/{}/cmdline", pid.trim())) {
        Ok(cmdline) => String::from_utf8_lossy(&cmdline).replace('\0', " "),
        Err(_) => {
            let ps = std::process::Command::new("ps")
                .args(["-o", "command=", "-p", pid.trim()])
                .output()
                .unwrap();
            String::from_utf8_lossy(&ps.stdout).into_owned()
        }
    };
    assert!(!argv.trim().is_empty(), "ps saw the worker");
    assert!(
        !argv.contains("from-dotenv") && !argv.contains("release_dir"),
        "the spec is not in argv: {argv}"
    );
}
