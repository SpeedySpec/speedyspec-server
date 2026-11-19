//! The `php-worker` role: boots PHP once, then serves FastCGI requests on its socket, one at a time.

use std::collections::BTreeMap;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_fastcgi::Responder;
use speedyspec_php::{BootOptions, NativeArea, Php, WebRequest};
use speedyspec_wp_hooks::HookStore;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::{HOOK_STORE, PhpHookStore, WpHooksArea};

/// What a worker is started with (as JSON, `php-worker --spec`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerSpec {
    pub app: String,
    pub socket: PathBuf,
    /// The release's root.
    pub release_dir: PathBuf,
    pub site_dir: Option<PathBuf>,
    /// The release's `php.ini` settings, applied at the start of every request.
    pub ini: BTreeMap<String, String>,
    /// `app.<id>.php.bindings`.
    pub bindings: Vec<(String, String)>,
}

/// A PHP string literal.
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// PHP run at the start of every request, before the script: the release's INI settings and the site directory's
/// `wp-content`.
fn preload(spec: &WorkerSpec) -> String {
    let mut code = String::from("<?php\n");
    for (key, value) in &spec.ini {
        code.push_str(&format!("ini_set({}, {});\n", quote(key), quote(value)));
    }
    if let Some(site) = &spec.site_dir {
        let content = site.join("wp-content");
        code.push_str(&format!(
            "if (!defined('WP_CONTENT_DIR')) {{ define('WP_CONTENT_DIR', {}); }}\n",
            quote(&content.display().to_string())
        ));
        code.push_str(
            "if (!defined('WP_CONTENT_URL') && isset($_SERVER['HTTP_HOST'])) { define('WP_CONTENT_URL', \
             ((($_SERVER['HTTPS'] ?? '') === 'on') ? 'https' : 'http') . '://' . $_SERVER['HTTP_HOST'] . \
             ($_SERVER['SPEEDYSPEC_MOUNT'] ?? '/') . 'wp-content'); }\n",
        );
    }
    code
}

/// `KEY=value` lines of `site_dir/.env`, nothing evaluated; comments and blank lines skipped.
pub fn read_env(site: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(site.join(".env")) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

/// One FastCGI request as PHP sees it.
struct FastCgiRequest<'a> {
    responder: &'a mut Responder<UnixStream>,
    params: Vec<(String, String)>,
    broken: bool,
}

impl WebRequest for FastCgiRequest<'_> {
    fn params(&self) -> &[(String, String)] {
        &self.params
    }

    fn read_body(&mut self, buf: &mut [u8]) -> usize {
        self.responder.read_stdin(buf).unwrap_or(0)
    }

    fn send_headers(&mut self, status: u16, headers: &[String]) -> bool {
        let mut head = format!("Status: {status}\r\n");
        for header in headers {
            head.push_str(header);
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        self.write(head.as_bytes())
    }

    fn write(&mut self, data: &[u8]) -> bool {
        if self.broken {
            return false;
        }
        if self.responder.write_stdout(data).is_err() {
            self.broken = true;
        }
        !self.broken
    }
}

fn param(params: &[(String, String)], name: &str) -> Option<String> {
    params
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.clone())
}

/// Serves until the socket is closed or the worker is told to stop. Returns the process exit status.
pub fn run_worker(spec: WorkerSpec) -> i32 {
    if let Some(site) = &spec.site_dir {
        for (key, value) in read_env(site) {
            // SAFETY: the worker is single-threaded at this point, before PHP starts.
            unsafe { std::env::set_var(key, value) };
        }
    }
    let container = Rc::new(Container::new());
    let memory = Rc::new(MemoryHookStore::new());
    let hooks = WpHooksArea::new(container.clone(), memory);
    let container_area = ContainerArea::new(&spec.app, container.clone())
        .port::<dyn HookStore>(HOOK_STORE, |object| Rc::new(PhpHookStore::new(object)))
        .stateful(HOOK_STORE)
        .bindings(spec.bindings.clone());
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        speedyspec_php_ext::twins_area(),
        Rc::new(container_area),
        Rc::new(hooks),
    ];
    let preload = preload(&spec);
    let php = match Php::boot(BootOptions {
        hooks: Rc::new(speedyspec_hooks_memory::MemoryHooks::new()),
        preload: &[("speedyspec://worker", preload.as_str())],
        areas,
    }) {
        Ok(php) => php,
        Err(err) => {
            log("php_worker_boot_failed", &spec, &err.to_string());
            return 1;
        }
    };
    let _ = std::fs::remove_file(&spec.socket);
    let listener = match UnixListener::bind(&spec.socket) {
        Ok(listener) => listener,
        Err(err) => {
            log(
                "php_worker_boot_failed",
                &spec,
                &format!("{}: {err}", spec.socket.display()),
            );
            return 1;
        }
    };
    let release = std::fs::canonicalize(&spec.release_dir).unwrap_or(spec.release_dir.clone());
    let mut requests = 0u64;
    for connection in listener.incoming() {
        let Ok(stream) = connection else {
            continue;
        };
        let mut responder = Responder::new(stream);
        loop {
            let head = match responder.next_request() {
                Ok(Some(head)) => head,
                _ => break,
            };
            let params: Vec<(String, String)> = head
                .params
                .iter()
                .map(|(k, v)| {
                    (
                        String::from_utf8_lossy(k).into_owned(),
                        String::from_utf8_lossy(v).into_owned(),
                    )
                })
                .collect();
            let script = param(&params, "SCRIPT_FILENAME").map(PathBuf::from);
            let inside = script
                .as_ref()
                .and_then(|s| std::fs::canonicalize(s).ok())
                .filter(|s| s.starts_with(&release));
            let mut request = FastCgiRequest {
                responder: &mut responder,
                params,
                broken: false,
            };
            let (poisoned, peak) = match inside {
                Some(script) => {
                    let served = php.serve(&mut request, &script);
                    if let Err(err) = &served.result {
                        let _ = request.responder.write_stderr(err.to_string().as_bytes());
                    }
                    (served.poisoned, served.peak_memory)
                }
                None => {
                    request.send_headers(404, &["Content-Type: text/plain".to_string()]);
                    request.write(b"not found");
                    (false, 0)
                }
            };
            requests += 1;
            let report = format!(
                "memory={peak} requests={requests} poisoned={}",
                u8::from(poisoned)
            );
            let mut record = super::REPORT_PREFIX.to_vec();
            record.extend_from_slice(report.as_bytes());
            let _ = responder.write_stderr(&record);
            if responder.end_request(0).is_err() {
                break;
            }
            if poisoned {
                log(
                    "php_worker_poisoned",
                    &spec,
                    "a fatal error left the engine degraded",
                );
                return 0;
            }
            if !head.keep_conn {
                break;
            }
        }
    }
    0
}

fn log(event: &str, spec: &WorkerSpec, message: &str) {
    let line = serde_json::json!({ "event": event, "app": spec.app, "socket": spec.socket, "message": message });
    eprintln!("{line}");
}
