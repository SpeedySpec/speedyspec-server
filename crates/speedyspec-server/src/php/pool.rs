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

use speedyspec_runtime_contract::WorkerSpec;

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

/// How long a draining pool still lends its idle workers to requests routed to it just before it began draining (a
/// switch swaps the handler first, but a request may already hold the old one).
pub const DRAIN_GRACE: Duration = Duration::from_secs(1);

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

impl Drop for Worker {
    /// A worker no pool holds any more is stopped: a child process is not killed when its handle is dropped.
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
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
    /// When draining began.
    drain_started: Option<Instant>,
    /// The worker kept for the server's own calls, when idle. Never counted in `total`.
    internal: Option<Worker>,
    /// The internal worker is leased.
    internal_busy: bool,
    /// A replacement internal worker is starting.
    internal_starting: bool,
}

impl State {
    /// Draining, but still lending idle workers to requests already routed to this pool.
    fn in_grace(&self) -> bool {
        self.draining
            && self
                .drain_started
                .is_some_and(|at| at.elapsed() < DRAIN_GRACE)
    }
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
    /// Wakes internal callers (apart from `wake`, so neither kind of caller takes the other's wakeup).
    internal_wake: tokio::sync::Notify,
    next: AtomicU64,
    /// Tests start fake workers: `(socket, process)` for a worker id.
    spawner: Option<Spawner>,
}

type Spawner = Box<dyn Fn(u64) -> Result<(PathBuf, Child), String> + Send + Sync>;

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
    /// The pool's internal worker, rather than a visitor's.
    internal: bool,
}

impl Lease {
    pub fn socket(&self) -> &Path {
        &self.worker.as_ref().expect("a leased worker").socket
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let (Some(worker), Some(inner)) = (self.worker.take(), self.pool.upgrade()) {
            EmbeddedPhpPool { inner }.give_back(
                worker,
                Fate::Crashed("the request was abandoned".into()),
                self.internal,
            );
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

static NONCE: AtomicU64 = AtomicU64::new(0);

/// Makes `dir` readable and writable by the server's user only.
fn private(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// Creates a pool's own socket directory, private to the server's user: `run_dir/php/<app>/<version>/<pid>-<n>/`, so
/// two pools of one release (a handoff, a reactivation, a rollback while the old pool drains) never share sockets.
/// When `run_dir` does not exist (the server never creates it) or the path is too long for a socket, a fresh
/// `mkdtemp` directory under `/tmp` is used instead.
pub fn prepare_socket_dir(run_dir: &Path, app: &str, version: &str) -> Result<PathBuf, String> {
    let unique = format!(
        "{}-{}",
        std::process::id(),
        NONCE.fetch_add(1, Ordering::SeqCst)
    );
    let dir = run_dir.join("php").join(app).join(version).join(unique);
    let short = dir.join("worker-0000.sock").as_os_str().len() <= SOCKET_PATH_LIMIT;
    let wanted = if !short {
        Err("the path is too long for a socket".to_string())
    } else if !run_dir.is_dir() {
        Err(format!("{} does not exist", run_dir.display()))
    } else {
        std::fs::create_dir_all(&dir)
            .and_then(|()| private(&dir))
            .map_err(|err| err.to_string())
    };
    if wanted.is_ok() {
        return Ok(dir);
    }
    let mut template = b"/tmp/speedyspec-php-XXXXXX\0".to_vec();
    // SAFETY: a NUL-terminated, writable template; mkdtemp creates the directory with mode 0700.
    let made = unsafe { libc::mkdtemp(template.as_mut_ptr().cast()) };
    if made.is_null() {
        return Err(format!(
            "creating a socket directory under /tmp: {}",
            std::io::Error::last_os_error()
        ));
    }
    template.pop();
    let fallback = PathBuf::from(String::from_utf8_lossy(&template).into_owned());
    if let Err(why) = wanted {
        tracing::warn!(event = "php_socket_dir_fallback", wanted = %dir.display(), error = %why, used = %fallback.display());
    }
    Ok(fallback)
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
                drain_started: None,
                internal: None,
                internal_busy: false,
                internal_starting: false,
            }),
            wake: tokio::sync::Notify::new(),
            internal_wake: tokio::sync::Notify::new(),
            next: AtomicU64::new(0),
            spawner: None,
        });
        let pool = EmbeddedPhpPool { inner };
        for _ in 0..pool.inner.settings.min.max(1) {
            let worker = pool.spawn_worker()?;
            let mut state = pool.inner.state.lock().unwrap();
            state.total += 1;
            state.idle.push(worker);
        }
        let internal = pool.spawn_worker()?;
        pool.inner.state.lock().unwrap().internal = Some(internal);
        log(
            "php_pool_started",
            &pool.inner,
            serde_json::json!({ "workers": pool.inner.settings.min.max(1) }),
        );
        let weak = Arc::downgrade(&pool.inner);
        std::thread::spawn(move || reap(weak));
        Ok(pool)
    }

    /// A pool with no workers yet, whose workers `spawner` starts (tests).
    #[cfg(test)]
    fn with_spawner(
        settings: PoolSettings,
        spawner: impl Fn(u64) -> Result<(PathBuf, Child), String> + Send + Sync + 'static,
    ) -> EmbeddedPhpPool {
        EmbeddedPhpPool {
            inner: Arc::new(Inner {
                app: "test".into(),
                version: "0.0.0".into(),
                exe: PathBuf::new(),
                spec: WorkerSpec {
                    contract: speedyspec_runtime_contract::CONTRACT_MAJOR,
                    env: Vec::new(),
                    app: "test".into(),
                    socket: PathBuf::new(),
                    release_dir: PathBuf::new(),
                    site_dir: None,
                    ini: Default::default(),
                    bindings: Vec::new(),
                },
                dir: PathBuf::from("/nonexistent"),
                settings,
                state: Mutex::new(State {
                    idle: Vec::new(),
                    total: 0,
                    busy: 0,
                    draining: false,
                    drain_started: None,
                    internal: None,
                    internal_busy: false,
                    internal_starting: false,
                }),
                wake: tokio::sync::Notify::new(),
                internal_wake: tokio::sync::Notify::new(),
                next: AtomicU64::new(0),
                spawner: Some(Box::new(spawner)),
            }),
        }
    }

    /// Starts one worker and waits until it answers `GET_VALUES`.
    fn spawn_worker(&self) -> Result<Worker, String> {
        let inner = &self.inner;
        let id = inner.next.fetch_add(1, Ordering::SeqCst);
        if let Some(spawner) = &inner.spawner {
            let (socket, child) = spawner(id)?;
            return Ok(Worker {
                id,
                socket,
                child,
                requests: 0,
                idle_since: Instant::now(),
            });
        }
        let socket = inner.dir.join(format!("worker-{id:04}.sock"));
        let _ = std::fs::remove_file(&socket);
        let mut spec = inner.spec.clone();
        spec.socket = socket.clone();
        let json = serde_json::to_string(&spec).map_err(|err| err.to_string())?;
        let mut child = Command::new(&inner.exe)
            .arg("worker")
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
                if state.draining && !state.in_grace() {
                    return Err("the release is being retired".into());
                }
                while let Some(mut worker) = state.idle.pop() {
                    if worker.alive() {
                        state.busy += 1;
                        return Ok(Lease {
                            pool: Arc::downgrade(&self.inner),
                            worker: Some(worker),
                            internal: false,
                        });
                    }
                    state.total -= 1;
                    log(
                        "php_worker_exited",
                        &self.inner,
                        serde_json::json!({ "worker": worker.id, "reason": "exited while idle" }),
                    );
                }
                if state.draining {
                    // A draining pool never grows: a request routed here late waits for an idle worker or the end
                    // of the grace period.
                    false
                } else if state.total < self.inner.settings.max {
                    state.total += 1;
                    true
                } else {
                    false
                }
            };
            // The new worker starts on its own thread and joins the idle set; this request waits for it like any
            // other, so a request that gives up meanwhile leaves no slot or process behind.
            if grow {
                let pool = self.clone();
                std::thread::spawn(move || pool.replace());
            }
            notified.await;
        }
    }

    /// Leases the worker kept for the server's own calls (health probes, the gate's questions, migrations). It is
    /// never a visitor's, so a pool full of visitors cannot starve these calls; it waits only for another internal
    /// call. A missing or dead internal worker is started again.
    pub async fn acquire_internal(&self) -> Result<Lease, String> {
        loop {
            let notified = self.inner.internal_wake.notified();
            let start = {
                let mut state = self.inner.state.lock().unwrap();
                if state.draining {
                    return Err("the release is being retired".into());
                }
                if !state.internal_busy {
                    if let Some(mut worker) = state.internal.take() {
                        if worker.alive() {
                            state.internal_busy = true;
                            return Ok(Lease {
                                pool: Arc::downgrade(&self.inner),
                                worker: Some(worker),
                                internal: true,
                            });
                        }
                        log(
                            "php_worker_exited",
                            &self.inner,
                            serde_json::json!({ "worker": worker.id, "reason": "the internal worker exited while idle" }),
                        );
                    }
                }
                let start =
                    !state.internal_busy && state.internal.is_none() && !state.internal_starting;
                if start {
                    state.internal_starting = true;
                }
                start
            };
            if start {
                let pool = self.clone();
                std::thread::spawn(move || pool.replace_internal());
            }
            notified.await;
        }
    }

    /// Starts the internal worker again.
    fn replace_internal(&self) {
        let started = self.spawn_worker();
        let mut state = self.inner.state.lock().unwrap();
        state.internal_starting = false;
        match started {
            Ok(worker) if !state.draining => state.internal = Some(worker),
            Ok(worker) => {
                drop(state);
                terminate(worker);
            }
            Err(err) => {
                drop(state);
                log(
                    "php_worker_exited",
                    &self.inner,
                    serde_json::json!({ "reason": err, "internal": true }),
                );
            }
        }
        self.inner.internal_wake.notify_waiters();
    }

    /// Gives a leased worker back, with what became of it.
    pub fn release(&self, mut lease: Lease, fate: Fate) {
        if let Some(worker) = lease.worker.take() {
            self.give_back(worker, fate, lease.internal);
        }
    }

    /// Takes the internal worker back: kept when it answered and is healthy, otherwise stopped and started again.
    fn give_back_internal(&self, mut worker: Worker, fate: Fate) {
        let keep = matches!(
            fate,
            Fate::Answered {
                poisoned: false,
                ..
            }
        ) && worker.alive();
        if let Fate::Answered { requests, .. } = fate {
            worker.requests = requests;
        }
        let mut state = self.inner.state.lock().unwrap();
        state.internal_busy = false;
        if keep && !state.draining {
            state.internal = Some(worker);
            drop(state);
        } else {
            let restart = !state.draining;
            if restart {
                state.internal_starting = true;
            }
            drop(state);
            let _ = worker.child.kill();
            let _ = worker.child.wait();
            log(
                "php_worker_recycled",
                &self.inner,
                serde_json::json!({ "worker": worker.id, "reason": format!("{fate:?}"), "internal": true }),
            );
            if restart {
                let pool = self.clone();
                std::thread::spawn(move || pool.replace_internal());
            }
        }
        self.inner.internal_wake.notify_waiters();
    }

    fn give_back(&self, mut worker: Worker, fate: Fate, internal: bool) {
        if internal {
            return self.give_back_internal(worker, fate);
        }
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
            (_, None) if (!state.draining || state.in_grace()) && worker.alive() => {
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
                self.inner.wake.notify_waiters();
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
        {
            let mut state = self.inner.state.lock().unwrap();
            state.draining = true;
            state.drain_started.get_or_insert_with(Instant::now);
        }
        self.inner.wake.notify_waiters();
        self.inner.internal_wake.notify_waiters();
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            loop {
                let idle = {
                    let mut state = inner.state.lock().unwrap();
                    if state.busy == 0 && !state.internal_busy && !state.in_grace() {
                        let mut idle = std::mem::take(&mut state.idle);
                        idle.extend(state.internal.take());
                        Some(idle)
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
                    // A request still waiting here is refused now rather than at its own timeout.
                    inner.wake.notify_waiters();
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

    /// A pool whose workers are `sleep` processes that take `delay` to start.
    fn fake_pool(max: u32, delay: Duration) -> EmbeddedPhpPool {
        let settings = PoolSettings {
            min: 0,
            max,
            max_requests: 100,
            request_timeout: Duration::from_secs(5),
            memory_limit: u64::MAX,
            idle_timeout: Duration::from_secs(60),
        };
        EmbeddedPhpPool::with_spawner(settings, move |id| {
            std::thread::sleep(delay);
            let child = Command::new("sleep")
                .arg("30")
                .spawn()
                .map_err(|e| e.to_string())?;
            Ok((
                PathBuf::from(format!("/nonexistent/worker-{id}.sock")),
                child,
            ))
        })
    }

    /// A request that gives up while its worker is starting leaks no capacity and no process: the worker joins the
    /// idle set for the next request.
    #[tokio::test]
    async fn cancelled_acquire_leaks_nothing() {
        let pool = fake_pool(1, Duration::from_millis(300));
        let gave_up = tokio::time::timeout(Duration::from_millis(50), pool.acquire()).await;
        assert!(
            gave_up.is_err(),
            "the caller gave up while the worker started"
        );
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert_eq!(
            pool.counts(),
            (1, 1),
            "the started worker is idle, not counted busy"
        );
        let lease = tokio::time::timeout(Duration::from_millis(200), pool.acquire()).await;
        assert!(
            matches!(lease, Ok(Ok(_))),
            "the next request gets it at once"
        );
    }

    #[test]
    fn socket_dirs_are_private_and_unique_per_pool() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::Builder::new()
            .prefix("ss")
            .tempdir_in("/tmp")
            .unwrap();
        let one = prepare_socket_dir(dir.path(), "blog", "1.0.0").unwrap();
        let two = prepare_socket_dir(dir.path(), "blog", "1.0.0").unwrap();
        assert_ne!(one, two, "two pools of one release never share a directory");
        let missing = prepare_socket_dir(&dir.path().join("missing"), "blog", "1.0.0").unwrap();
        for path in [&one, &two, &missing] {
            let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{} is private", path.display());
        }
        let _ = std::fs::remove_dir_all(&missing);
    }

    #[test]
    fn socket_dir_falls_back_when_run_dir_is_unusable() {
        // Short enough that run_dir itself is usable for sockets.
        let dir = tempfile::Builder::new()
            .prefix("ss")
            .tempdir_in("/tmp")
            .unwrap();
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
        let missing = dir.path().join("no-such-run-dir");
        let fallback = prepare_socket_dir(&missing, "blog", "3.0.0").unwrap();
        assert!(!missing.exists(), "the server never creates run_dir");
        assert!(
            fallback
                .to_string_lossy()
                .starts_with("/tmp/speedyspec-php-"),
            "{}",
            fallback.display()
        );
        let _ = std::fs::remove_dir_all(&fallback);
    }

    /// The internal worker is apart from the visitors' workers: with every visitor worker busy, the server's own call
    /// still gets one, and it is not counted against `max`.
    #[tokio::test]
    async fn internal_worker_is_kept_apart() {
        let pool = fake_pool(1, Duration::from_millis(0));
        let visitor = tokio::time::timeout(Duration::from_secs(2), pool.acquire())
            .await
            .unwrap()
            .unwrap();
        let internal = tokio::time::timeout(Duration::from_secs(2), pool.acquire_internal()).await;
        assert!(
            matches!(internal, Ok(Ok(_))),
            "the internal call gets a worker at once"
        );
        let second = tokio::time::timeout(Duration::from_millis(200), pool.acquire()).await;
        assert!(second.is_err(), "a second visitor waits: max is 1");
        assert_eq!(pool.counts().0, 1, "only visitor workers are counted");
        drop(visitor);
    }

    /// A killed internal worker is replaced, and the next internal call gets the replacement.
    #[tokio::test]
    async fn killed_internal_worker_is_replaced() {
        let pool = fake_pool(1, Duration::from_millis(0));
        let lease = pool.acquire_internal().await.unwrap();
        pool.release(lease, Fate::TimedOut);
        let again = tokio::time::timeout(Duration::from_secs(2), pool.acquire_internal()).await;
        assert!(
            matches!(again, Ok(Ok(_))),
            "a replacement internal worker answers"
        );
    }

    /// A request routed to a release just before its pool began draining is still served by an idle worker, for
    /// the drain's grace period; after it, the pool refuses.
    #[tokio::test]
    async fn draining_pool_still_serves_requests_routed_to_it() {
        let pool = fake_pool(1, Duration::from_millis(0));
        let lease = pool.acquire().await.unwrap();
        pool.release(
            lease,
            Fate::Answered {
                memory: 0,
                requests: 1,
                poisoned: false,
            },
        );
        pool.drain();
        let straggler = tokio::time::timeout(Duration::from_millis(200), pool.acquire()).await;
        assert!(
            matches!(straggler, Ok(Ok(_))),
            "a request already routed here is served"
        );
        tokio::time::sleep(DRAIN_GRACE + Duration::from_millis(300)).await;
        assert!(
            pool.acquire().await.is_err(),
            "after the grace period the pool refuses"
        );
    }
}
