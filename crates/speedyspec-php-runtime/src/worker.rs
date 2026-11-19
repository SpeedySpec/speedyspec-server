//! `speedyspec-php-runtime worker`: boots PHP once, then serves FastCGI requests on its socket, one at a time.

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_fastcgi::Responder;
use speedyspec_php::{BootOptions, NativeArea, Php, WebRequest};
use speedyspec_runtime_contract::{WorkerReport, WorkerSpec};
use speedyspec_wp_hooks::HookStore;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::{HOOK_STORE, PhpHookStore, WpHooksArea};

/// A PHP string literal.
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// PHP run at the start of every request, before the script: `WP_CONTENT_DIR` is the site directory's
/// `wp-content`. `WP_CONTENT_URL` is left to WordPress, which derives it from `siteurl` (building it from the request's
/// Host would let any client choose it). The release's INI settings are applied at boot ([`run_worker`]).
fn preload(spec: &WorkerSpec) -> String {
    let mut code = String::from("<?php\n");
    if let Some(site) = &spec.site_dir {
        let content = site.join("wp-content");
        code.push_str(&format!(
            "if (!defined('WP_CONTENT_DIR')) {{ define('WP_CONTENT_DIR', {}); }}\n",
            quote(&content.display().to_string())
        ));
    }
    code
}

/// The CLI's plugin registry (`SpeedySpec\Hooks\Hook`), which the runtime always registers, emptied at the end of
/// every request: the PHP callbacks it holds belong to that request and must not outlive it.
struct RequestHooks(std::cell::RefCell<Rc<speedyspec_hooks_memory::MemoryHooks>>);

impl RequestHooks {
    fn current(&self) -> Rc<speedyspec_hooks_memory::MemoryHooks> {
        self.0.borrow().clone()
    }
}

impl speedyspec_hooks::Hooks for RequestHooks {
    fn add(
        &self,
        hook: &str,
        callback: Rc<speedyspec_hooks::Callback>,
        options: speedyspec_hooks::AddOptions,
    ) -> Result<speedyspec_hooks::CallbackId, speedyspec_hooks::HookError> {
        self.current().add(hook, callback, options)
    }
    fn remove(
        &self,
        hook: &str,
        id: speedyspec_hooks::CallbackId,
    ) -> Result<bool, speedyspec_hooks::HookError> {
        self.current().remove(hook, id)
    }
    fn remove_keyed(
        &self,
        hook: &str,
        key: &str,
        priority: i64,
    ) -> Result<bool, speedyspec_hooks::HookError> {
        self.current().remove_keyed(hook, key, priority)
    }
    fn has(&self, hook: &str) -> Result<bool, speedyspec_hooks::HookError> {
        self.current().has(hook)
    }
    fn has_keyed(&self, hook: &str, key: &str) -> Result<bool, speedyspec_hooks::HookError> {
        self.current().has_keyed(hook, key)
    }
    fn run(
        &self,
        hook: &str,
        args: &[speedyspec_hooks::Value],
    ) -> Result<(), speedyspec_hooks::HookError> {
        self.current().run(hook, args)
    }
    fn filter(
        &self,
        hook: &str,
        value: speedyspec_hooks::Value,
        args: &[speedyspec_hooks::Value],
    ) -> Result<speedyspec_hooks::Value, speedyspec_hooks::HookError> {
        self.current().filter(hook, value, args)
    }
    fn ran(&self, hook: &str) -> Result<u64, speedyspec_hooks::HookError> {
        self.current().ran(hook)
    }
}

/// Empties [`RequestHooks`] as each request ends (also after a fatal error).
struct RequestHooksArea(Rc<RequestHooks>);

impl NativeArea for RequestHooksArea {
    fn request_end(&self) {
        let old = self
            .0
            .0
            .replace(Rc::new(speedyspec_hooks_memory::MemoryHooks::new()));
        drop(old);
    }
}

/// Exits when the server that started this worker is gone (the worker is then a child of init).
fn exit_with_parent() {
    // SAFETY: getppid has no preconditions.
    let parent = unsafe { libc::getppid() };
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            // SAFETY: as above.
            if unsafe { libc::getppid() } != parent {
                std::process::exit(0);
            }
        }
    });
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

    /// Fills `buf` unless the body ends first: PHP takes a short read as the end of the body.
    fn read_body(&mut self, buf: &mut [u8]) -> usize {
        let mut filled = 0;
        while filled < buf.len() {
            match self.responder.read_stdin(&mut buf[filled..]) {
                Ok(0) | Err(_) => break,
                Ok(n) => filled += n,
            }
        }
        filled
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
    if let Err(err) = spec.check() {
        log("php_worker_contract_refused", &spec, &err);
        return 2;
    }
    let site_env = spec.site_dir.as_deref().map(read_env).unwrap_or_default();
    for (key, value) in site_env.into_iter().chain(spec.env.iter().cloned()) {
        // SAFETY: the worker is single-threaded at this point, before PHP starts.
        unsafe { std::env::set_var(key, value) };
    }
    exit_with_parent();
    let ini: Vec<(String, String)> = spec
        .ini
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if let Err(err) = speedyspec_php::set_ini(&ini) {
        log("php_worker_boot_failed", &spec, &err);
        return 1;
    }
    let legacy = Rc::new(RequestHooks(std::cell::RefCell::new(Rc::new(
        speedyspec_hooks_memory::MemoryHooks::new(),
    ))));
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
        Rc::new(RequestHooksArea(legacy.clone())),
    ];
    let preload = preload(&spec);
    let php = match Php::boot(BootOptions {
        hooks: legacy,
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
    {
        use std::os::unix::fs::PermissionsExt;
        // Only the server (the same user) may send requests: a request's parameters are trusted ($_SERVER).
        let _ = std::fs::set_permissions(&spec.socket, std::fs::Permissions::from_mode(0o600));
    }
    // Scripts run from the release, or from the site directory's wp-content (plugins and themes installed there).
    let roots: Vec<PathBuf> = std::iter::once(spec.release_dir.clone())
        .chain(spec.site_dir.iter().map(|site| site.join("wp-content")))
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .collect();
    let mut requests = 0u64;
    for connection in listener.incoming() {
        let Ok(stream) = connection else {
            continue;
        };
        let mut responder = Responder::new(stream);
        while let Ok(Some(head)) = responder.next_request() {
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
                .filter(|s| roots.iter().any(|root| s.starts_with(root)));
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
            let report = WorkerReport {
                memory: peak,
                requests,
                poisoned,
            };
            let _ = responder.write_stderr(&report.encode());
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
