//! `EmbeddedPhpPool`: one release's `php-worker` processes, each on its own socket under
//! `run_dir/php/<app>/<version>/`.
//!
//! A request leases one idle worker (starting another, up to `max`, when none is idle; waiting when `max` are
//! busy) and gives it back with what became of it: kept, recycled (`max_requests`, `memory_limit`, a poisoned
//! engine), or killed (it crashed, or ran past `request_timeout`). Idle workers above `min` exit after
//! `idle_timeout`; a worker that dies is replaced to keep `min`. `drain` stops new leases, waits for the open ones,
//! then stops every worker. Every event is a log line.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use speedyspec_fastcgi::{Record, RecordType, encode_pairs};

use super::worker::WorkerSpec;

/// The pool's sizing and limits (from `app.<id>.php`).
#[derive(Clone, Debug)]
pub struct PoolSettings {
    pub min: u32,
    pub max: u32,
    pub max_requests: u64,
    pub request_timeout: Duration,
    pub memory_limit: u64,
    pub idle_timeout: Duration,
}

impl PoolSettings {
    pub fn from_config(config: &crate::config::PhpConfig) -> PoolSettings {
        PoolSettings {
            min: config.workers.min,
            max: config.workers.max,
            max_requests: config.max_requests,
            request_timeout: config.request_timeout,
            memory_limit: config.memory_limit,
            idle_timeout: config.idle_timeout,
        }
    }
}

/// How long a new worker may take to answer `GET_VALUES`.
const READY_TIMEOUT: Duration = Duration::from_secs(20);
/// macOS and Linux limit a socket path to about 104 bytes.
const SOCKET_PATH_LIMIT: usize = 100;

/// One worker process.
pub struct Worker {
    pub id: u64,
    pub socket: PathBuf,
    child: Child,
    pub requests: u64,
    idle_since: Instant,
}

impl Worker {
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }
}

/// What became of a leased worker.
#[derive(Clone, Debug)]
pub enum Fate {
    /// It answered; its after-request report.
    Answered {
        memory: u64,
        requests: u64,
        poisoned: bool,
    },
    /// It died during the request.
    Crashed(String),
    /// It ran past `request_timeout` and is killed.
    TimedOut,
}

struct State {
    idle: Vec<Worker>,
    total: u32,
    busy: u32,
    draining: bool,
}

struct Inner {
    app: String,
    version: String,
    exe: PathBuf,
    spec: WorkerSpec,
    dir: PathBuf,
    settings: PoolSettings,
    state: Mutex<State>,
    wake: tokio::sync::Notify,
    next: AtomicU64,
}

/// See the module docs.
#[derive(Clone)]
pub struct EmbeddedPhpPool {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for EmbeddedPhpPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "EmbeddedPhpPool({} {})",
            self.inner.app, self.inner.version
        )
    }
}

/// A worker leased for one request. Dropped without [`EmbeddedPhpPool::release`], the worker is killed.
pub struct Lease {
    pool: Weak<Inner>,
    worker: Option<Worker>,
}

impl Lease {
    pub fn socket(&self) -> &Path {
        &self.worker.as_ref().expect("a leased worker").socket
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let (Some(worker), Some(inner)) = (self.worker.take(), self.pool.upgrade()) {
            EmbeddedPhpPool { inner }
                .give_back(worker, Fate::Crashed("the request was abandoned".into()));
        }
    }
}

fn log(event: &str, inner: &Inner, fields: serde_json::Value) {
    let mut line = serde_json::json!({ "app": inner.app, "version": inner.version });
    if let (Some(line), Some(fields)) = (line.as_object_mut(), fields.as_object()) {
        for (k, v) in fields {
            line.insert(k.clone(), v.clone());
        }
    }
    tracing::info!(event, details = %line);
}

/// Where a release's worker sockets go: `run_dir/php/<app>/<version>/`, or a short directory under `/tmp` when that
/// path would be too long for a socket.
pub fn socket_dir(run_dir: &Path, app: &str, version: &str) -> PathBuf {
    let dir = run_dir.join("php").join(app).join(version);
    if dir.join("worker-0000.sock").as_os_str().len() <= SOCKET_PATH_LIMIT {
        return dir;
    }
    PathBuf::from(format!(
        "/tmp/speedyspec-php-{}-{app}-{version}",
        std::process::id()
    ))
}

/// Creates the release's socket directory: [`socket_dir`], or a private directory under `/tmp` when `run_dir`
/// cannot be used (the server already runs without its lock and control socket then, with a warning).
pub fn prepare_socket_dir(run_dir: &Path, app: &str, version: &str) -> Result<PathBuf, String> {
    let dir = socket_dir(run_dir, app, version);
    match std::fs::create_dir_all(&dir) {
        Ok(()) => Ok(dir),
        Err(err) => {
            let fallback = PathBuf::from(format!(
                "/tmp/speedyspec-php-{}-{app}-{version}",
                std::process::id()
            ));
            tracing::warn!(event = "php_socket_dir_fallback", wanted = %dir.display(), error = %err, used = %fallback.display());
            std::fs::create_dir_all(&fallback)
                .map_err(|err| format!("{}: {err}", fallback.display()))?;
            Ok(fallback)
        }
    }
}

impl EmbeddedPhpPool {
    /// Starts the pool's `min` workers (at least one) and waits for them to answer.
    pub fn start(
        exe: PathBuf,
        spec: WorkerSpec,
        dir: PathBuf,
        settings: PoolSettings,
        version: String,
    ) -> Result<EmbeddedPhpPool, String> {
        std::fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
        let inner = Arc::new(Inner {
            app: spec.app.clone(),
            version,
            exe,
            spec,
            dir,
            settings,
            state: Mutex::new(State {
                idle: Vec::new(),
                total: 0,
                busy: 0,
                draining: false,
            }),
            wake: tokio::sync::Notify::new(),
            next: AtomicU64::new(0),
        });
        let pool = EmbeddedPhpPool { inner };
        for _ in 0..pool.inner.settings.min.max(1) {
            let worker = pool.spawn_worker()?;
            let mut state = pool.inner.state.lock().unwrap();
            state.total += 1;
            state.idle.push(worker);
        }
        log(
            "php_pool_started",
            &pool.inner,
            serde_json::json!({ "workers": pool.inner.settings.min.max(1) }),
        );
        let weak = Arc::downgrade(&pool.inner);
        std::thread::spawn(move || reap(weak));
        Ok(pool)
    }

    /// Starts one worker and waits until it answers `GET_VALUES`.
    fn spawn_worker(&self) -> Result<Worker, String> {
        let inner = &self.inner;
        let id = inner.next.fetch_add(1, Ordering::SeqCst);
        let socket = inner.dir.join(format!("worker-{id:04}.sock"));
        let _ = std::fs::remove_file(&socket);
        let mut spec = inner.spec.clone();
        spec.socket = socket.clone();
        let json = serde_json::to_string(&spec).map_err(|err| err.to_string())?;
        let mut child = Command::new(&inner.exe)
            .arg("php-worker")
            .arg("--spec")
            .arg(json)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|err| format!("starting {}: {err}", inner.exe.display()))?;
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!("the PHP worker exited while starting ({status})"));
            }
            if ready(&socket) {
                break;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "the PHP worker did not answer on {} within {READY_TIMEOUT:?}",
                    socket.display()
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        log(
            "php_worker_started",
            inner,
            serde_json::json!({ "worker": id, "pid": child.id() }),
        );
        Ok(Worker {
            id,
            socket,
            child,
            requests: 0,
            idle_since: Instant::now(),
        })
    }

    /// Leases an idle worker, starting one when none is idle and fewer than `max` run, or waiting.
    pub async fn acquire(&self) -> Result<Lease, String> {
        loop {
            let notified = self.inner.wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let grow = {
                let mut state = self.inner.state.lock().unwrap();
                if state.draining {
                    return Err("the release is being retired".into());
                }
                while let Some(mut worker) = state.idle.pop() {
                    if worker.alive() {
                        state.busy += 1;
                        return Ok(Lease {
                            pool: Arc::downgrade(&self.inner),
                            worker: Some(worker),
                        });
                    }
                    state.total -= 1;
                    log(
                        "php_worker_exited",
                        &self.inner,
                        serde_json::json!({ "worker": worker.id, "reason": "exited while idle" }),
                    );
                }
                if state.total < self.inner.settings.max {
                    state.total += 1;
                    state.busy += 1;
                    true
                } else {
                    false
                }
            };
            if grow {
                let pool = self.clone();
                let started = tokio::task::spawn_blocking(move || pool.spawn_worker())
                    .await
                    .map_err(|e| e.to_string())?;
                return match started {
                    Ok(worker) => Ok(Lease {
                        pool: Arc::downgrade(&self.inner),
                        worker: Some(worker),
                    }),
                    Err(err) => {
                        let mut state = self.inner.state.lock().unwrap();
                        state.total -= 1;
                        state.busy -= 1;
                        Err(err)
                    }
                };
            }
            notified.await;
        }
    }

    /// Gives a leased worker back, with what became of it.
    pub fn release(&self, mut lease: Lease, fate: Fate) {
        if let Some(worker) = lease.worker.take() {
            self.give_back(worker, fate);
        }
    }

    fn give_back(&self, mut worker: Worker, fate: Fate) {
        let inner = &self.inner;
        let settings = &inner.settings;
        let recycle = match &fate {
            Fate::Answered { poisoned: true, .. } => Some("the engine is poisoned"),
            Fate::Answered { memory, .. } if *memory > settings.memory_limit => {
                Some("memory_limit")
            }
            Fate::Answered { requests, .. } if *requests >= settings.max_requests => {
                Some("max_requests")
            }
            Fate::Answered { .. } => None,
            Fate::Crashed(_) | Fate::TimedOut => Some("gone"),
        };
        if let Fate::Answered { requests, .. } = fate {
            worker.requests = requests;
        }
        let mut state = inner.state.lock().unwrap();
        state.busy -= 1;
        match (&fate, recycle) {
            (_, None) if !state.draining && worker.alive() => {
                worker.idle_since = Instant::now();
                state.idle.push(worker);
            }
            (Fate::Crashed(why), _) => {
                state.total -= 1;
                let _ = worker.child.kill();
                let status = worker.child.wait().ok().map(|s| s.to_string());
                log(
                    "php_worker_exited",
                    inner,
                    serde_json::json!({ "worker": worker.id, "reason": why, "status": status }),
                );
            }
            (Fate::TimedOut, _) => {
                state.total -= 1;
                let _ = worker.child.kill();
                let _ = worker.child.wait();
                log(
                    "php_request_timeout",
                    inner,
                    serde_json::json!({ "worker": worker.id, "pid": worker.pid() }),
                );
            }
            (_, reason) => {
                state.total -= 1;
                let reason = reason.unwrap_or("the release is being retired");
                log(
                    "php_worker_recycled",
                    inner,
                    serde_json::json!({ "worker": worker.id, "reason": reason, "requests": worker.requests }),
                );
                terminate(worker);
            }
        }
        let short = !state.draining && state.total < settings.min.max(1);
        if short {
            state.total += 1;
        }
        drop(state);
        inner.wake.notify_one();
        if short {
            let pool = self.clone();
            std::thread::spawn(move || pool.replace());
        }
    }

    /// Starts a replacement worker (its slot is already counted).
    fn replace(&self) {
        match self.spawn_worker() {
            Ok(worker) => {
                let mut state = self.inner.state.lock().unwrap();
                if state.draining {
                    state.total -= 1;
                    drop(state);
                    terminate(worker);
                } else {
                    state.idle.push(worker);
                    drop(state);
                    self.inner.wake.notify_one();
                }
            }
            Err(err) => {
                self.inner.state.lock().unwrap().total -= 1;
                log(
                    "php_worker_exited",
                    &self.inner,
                    serde_json::json!({ "reason": err }),
                );
            }
        }
    }

    /// Stops new leases, waits (on a thread of its own) for the open ones, then stops every worker.
    pub fn drain(&self) {
        self.inner.state.lock().unwrap().draining = true;
        self.inner.wake.notify_waiters();
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            loop {
                let idle = {
                    let mut state = inner.state.lock().unwrap();
                    if state.busy == 0 {
                        Some(std::mem::take(&mut state.idle))
                    } else {
                        None
                    }
                };
                if let Some(idle) = idle {
                    let count = idle.len();
                    for worker in idle {
                        terminate(worker);
                    }
                    log(
                        "php_pool_drained",
                        &inner,
                        serde_json::json!({ "workers": count }),
                    );
                    let _ = std::fs::remove_dir_all(&inner.dir);
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
    }

    /// Workers running and idle now.
    pub fn counts(&self) -> (u32, usize) {
        let state = self.inner.state.lock().unwrap();
        (state.total, state.idle.len())
    }

    pub fn request_timeout(&self) -> Duration {
        self.inner.settings.request_timeout
    }
}

/// Asks the worker on `socket` for its values; true when it answers.
fn ready(socket: &Path) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let ask = Record::new(
        RecordType::GetValues,
        0,
        encode_pairs(&[(b"FCGI_MAX_CONNS".as_slice(), b"".as_slice())]),
    );
    if stream.write_all(&ask.encode()).is_err() {
        return false;
    }
    let mut reader = &mut stream as &mut dyn Read;
    matches!(Record::read_from(&mut reader), Ok(Some(record)) if record.kind == RecordType::GetValuesResult)
}

/// Asks a worker to exit (SIGTERM), and reaps it in the background.
fn terminate(mut worker: Worker) {
    // SAFETY: signalling our own child process by its pid.
    unsafe { libc::kill(worker.pid() as libc::pid_t, libc::SIGTERM) };
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if !matches!(worker.child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = worker.child.kill();
        let _ = worker.child.wait();
    });
}

/// Retires idle workers above `min` after `idle_timeout`, and notices idle workers that died.
fn reap(weak: Weak<Inner>) {
    loop {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        let tick = (inner.settings.idle_timeout / 4)
            .clamp(Duration::from_millis(50), Duration::from_secs(1));
        let mut retired = Vec::new();
        let mut short = false;
        {
            let mut state = inner.state.lock().unwrap();
            if state.draining {
                return;
            }
            let mut kept = Vec::new();
            for mut worker in std::mem::take(&mut state.idle) {
                if !worker.alive() {
                    state.total -= 1;
                    log(
                        "php_worker_exited",
                        &inner,
                        serde_json::json!({ "worker": worker.id, "reason": "exited while idle" }),
                    );
                } else if state.total > inner.settings.min
                    && worker.idle_since.elapsed() >= inner.settings.idle_timeout
                {
                    state.total -= 1;
                    retired.push(worker);
                } else {
                    kept.push(worker);
                }
            }
            state.idle = kept;
            if state.total < inner.settings.min.max(1) {
                state.total += 1;
                short = true;
            }
        }
        for worker in retired {
            log(
                "php_worker_recycled",
                &inner,
                serde_json::json!({ "worker": worker.id, "reason": "idle_timeout" }),
            );
            terminate(worker);
        }
        if short {
            EmbeddedPhpPool {
                inner: inner.clone(),
            }
            .replace();
        }
        drop(inner);
        std::thread::sleep(tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_dir_falls_back_when_run_dir_is_unusable() {
        let dir = tempfile::tempdir().unwrap();
        let usable = prepare_socket_dir(dir.path(), "blog", "1.0.0").unwrap();
        assert!(usable.starts_with(dir.path()) && usable.is_dir());
        let blocked = dir.path().join("file");
        std::fs::write(&blocked, "not a directory").unwrap();
        let fallback = prepare_socket_dir(&blocked, "blog", "2.0.0").unwrap();
        assert!(
            fallback.starts_with("/tmp") && fallback.is_dir(),
            "{}",
            fallback.display()
        );
        let _ = std::fs::remove_dir_all(&fallback);
    }
}
