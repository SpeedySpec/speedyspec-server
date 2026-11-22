//! `speedyspec-php-runtime worker`: boots PHP once, then serves FastCGI requests on its socket, one at a time.

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_fastcgi::Responder;
use speedyspec_hooks::Value;
use speedyspec_php::{BootOptions, NativeArea, Php, WebRequest};
use speedyspec_runtime_contract::{WorkerReport, WorkerSpec};
use speedyspec_wp_crypto_php::CryptoArea;
use speedyspec_wp_crypto_sodium::Sodium;
use speedyspec_wp_hooks::{HookApi, HookStore};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::{HOOK_STORE, PhpHookStore, WpHooksArea};
use speedyspec_wp_password_php::PasswordArea;

/// WordPress's password functions, with WordPress's rules bound in `container` under their pluggable ports.
fn password_area(hooks: Rc<HookApi>, container: &Rc<Container>, sodium: Sodium) -> PasswordArea {
    let area = PasswordArea::new(hooks, container.clone(), Rc::new(sodium));
    area.bind_defaults();
    area
}

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
    for (key, value) in &spec.env {
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
    // WordPress needs libsodium: the runtime ships it, and refuses to serve without it.
    let sodium = match Sodium::init() {
        Ok(sodium) => sodium,
        Err(err) => {
            log("php_worker_sodium_missing", &spec, &err);
            return 1;
        }
    };
    let database_config = match crate::database::database_config(|key| std::env::var(key).ok()) {
        Ok(config) => config,
        Err(err) => {
            log("php_worker_boot_failed", &spec, &err);
            return 1;
        }
    };
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
    let container_area = PasswordArea::ports(container_area);
    let hook_api = hooks.api();
    let crypto = CryptoArea::new(hook_api.clone(), sodium);
    let password = password_area(hook_api.clone(), &container, sodium);
    let mut areas: Vec<Rc<dyn NativeArea>> = vec![
        speedyspec_php_ext::twins_area(),
        Rc::new(container_area),
        Rc::new(hooks),
        Rc::new(RequestHooksArea(legacy.clone())),
    ];
    // With database credentials: the database, object cache and options areas, for the worker's life.
    let database = match database_config {
        Some(config) => match crate::database::boot_database(config, &container, &hook_api) {
            Ok((database, database_areas)) => {
                areas.extend(database_areas);
                Some(database)
            }
            Err(failure) => {
                let (event, message) = failure.event();
                log(event, &spec, message);
                return 1;
            }
        },
        None => None,
    };
    areas.push(Rc::new(crypto));
    areas.push(Rc::new(password));
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
    let loaded = matches!(
        php.eval(
            "speedyspec://worker/sodium",
            "return extension_loaded('sodium');"
        ),
        Ok(Value::Bool(true))
    );
    if let Some(problem) = sodium_problem(loaded) {
        log("php_worker_sodium_missing", &spec, &problem);
        return 1;
    }
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
            // A db.php drop-in would replace the native wpdb: refused, every request, before PHP runs.
            let drop_in = match (&database, &spec.site_dir) {
                (Some(_), Some(site)) => {
                    speedyspec_wp_db_php::WpDbArea::check_drop_in(&site.join("wp-content")).err()
                }
                _ => None,
            };
            let (poisoned, peak) = match (drop_in, inside) {
                (Some(err), _) => {
                    log("php_worker_drop_in_refused", &spec, &err.to_string());
                    request.send_headers(500, &["Content-Type: text/plain".to_string()]);
                    request.write(err.to_string().as_bytes());
                    (false, 0)
                }
                (None, Some(script)) => {
                    let served = php.serve(&mut request, &script);
                    if let Err(err) = &served.result {
                        let _ = request.responder.write_stderr(err.to_string().as_bytes());
                    }
                    (served.poisoned, served.peak_memory)
                }
                (None, None) => {
                    request.send_headers(404, &["Content-Type: text/plain".to_string()]);
                    request.write(b"not found");
                    (false, 0)
                }
            };
            // Connections live for the worker; a transaction a request left open does not.
            if let Some(database) = &database
                && let Err(err) = database.registry.reset_all()
            {
                log("php_worker_open_transaction", &spec, &err.to_string());
            }
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

/// Why this worker must not serve, if PHP's ext-sodium is missing.
///
/// Only presence is checked. PHP and the Rust port can't run different libsodium versions: the runtime is one static
/// binary, and `speedyspec-php`'s build script refuses a `SODIUM_LIB_DIR` other than the static PHP's own `lib`, so
/// both link the same `libsodium.a`.
fn sodium_problem(loaded: bool) -> Option<String> {
    (!loaded).then(|| {
        "PHP has no ext-sodium; WordPress needs it and this runtime must ship it".to_string()
    })
}

fn log(event: &str, spec: &WorkerSpec, message: &str) {
    let line = serde_json::json!({ "event": event, "app": spec.app, "socket": spec.socket, "message": message });
    eprintln!("{line}");
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use speedyspec_container::Container;
    use speedyspec_hooks::Value;
    use speedyspec_wp_crypto_sodium::Sodium;
    use speedyspec_wp_hooks_memory::MemoryHookStore;
    use speedyspec_wp_hooks_php::WpHooksArea;
    use speedyspec_wp_password_contracts::{
        PASSWORD_CHECKER, PASSWORD_GENERATOR, PASSWORD_HASHER, PasswordChecker, PasswordGenerator,
        PasswordHasher, RANDOM_NUMBER, REHASH_POLICY, RandomNumber, RehashPolicy,
    };

    use super::{password_area, sodium_problem};

    #[test]
    fn password_ports_are_bound() {
        let container = Rc::new(Container::new());
        let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
        let _area = password_area(hooks.api(), &container, Sodium::init().unwrap());
        assert!(
            container
                .make_port::<dyn PasswordHasher>(PASSWORD_HASHER)
                .is_ok()
        );
        assert!(
            container
                .make_port::<dyn PasswordChecker>(PASSWORD_CHECKER)
                .is_ok()
        );
        assert!(
            container
                .make_port::<dyn RehashPolicy>(REHASH_POLICY)
                .is_ok()
        );
        assert!(
            container
                .make_port::<dyn PasswordGenerator>(PASSWORD_GENERATOR)
                .is_ok()
        );
        let numbers = container
            .make_port::<dyn RandomNumber>(RANDOM_NUMBER)
            .unwrap();
        assert!(matches!(numbers.rand(3, 3), Ok(Value::Int(3))));
    }

    #[test]
    fn sodium_must_be_loaded() {
        assert_eq!(sodium_problem(true), None);
        assert_eq!(
            sodium_problem(false).as_deref(),
            Some("PHP has no ext-sodium; WordPress needs it and this runtime must ship it")
        );
    }
}
