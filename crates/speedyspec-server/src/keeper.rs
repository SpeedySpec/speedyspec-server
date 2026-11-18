//! The `run` role: the keeper. It binds every socket once, holds `server.lock`, serves `keeper.sock`, and runs the
//! server in a child generation that it restarts when it crashes. It serves no traffic itself.

use std::collections::HashMap;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use semver::Version;
use serde_json::Value;
use speedyspec_server_handoff::{
    AsyncChannel, Channel, GenerationRole, PROTOCOL, SocketInfo, SocketKind, ToGeneration, ToKeeper,
};
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::binary::own_version;
use crate::boot::bind;
use crate::config::{Config, Mode};
use crate::control::{self, LineHandler};
use crate::generation::absolute;

/// Crash-restart back-off: from 1 s, doubling, to at most 1 minute; reset after a minute of uptime.
const BACKOFF_START: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(60);

/// What a generation's channel reader reports to the supervisor.
#[allow(dead_code)] // `Upgrade` and `Reload` are acted on by the upgrade path.
#[derive(Debug)]
pub(crate) enum GenEvent {
    Ready(Version),
    UdpReleased,
    Upgrade { version: Version, binary: PathBuf },
    Reload,
    Exiting,
    Closed,
}

/// One running generation, as the keeper sees it.
pub(crate) struct Generation {
    pub(crate) pid: u32,
    pub(crate) version: std::sync::Mutex<Option<Version>>,
    pub(crate) on_trial: std::sync::atomic::AtomicBool,
    /// Set when the channel closes, before pending replies are failed.
    closed: Arc<std::sync::atomic::AtomicBool>,
    channel: Arc<AsyncChannel>,
    replies: Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<String>>>>,
    pub(crate) child: Mutex<ChildProc>,
}

impl Generation {
    pub(crate) async fn send(&self, message: &ToGeneration) {
        if let Err(err) = self.channel.send(message, &[]).await {
            tracing::warn!(event = "handoff_send_failed", pid = self.pid, error = %err);
        }
    }
}

/// The sockets the keeper owns, with what each one is.
pub(crate) struct Sockets {
    pub(crate) list: Vec<(SocketInfo, OwnedFd)>,
}

/// Everything the supervisor and the control handler share.
pub(crate) struct Keeper {
    pub(crate) config_path: PathBuf,
    pub(crate) config: Config,
    pub(crate) sockets: Sockets,
    pub(crate) active: Mutex<Option<Arc<Generation>>>,
    /// Woken whenever a generation becomes active.
    pub(crate) activated: tokio::sync::Notify,
    next_id: AtomicU64,
    commands: std::sync::OnceLock<mpsc::UnboundedSender<KeeperCommand>>,
}

impl Keeper {
    /// Starts a generation of `binary`, hands it the sockets, and returns it with its event stream.
    pub(crate) async fn spawn(
        &self,
        binary: &Path,
    ) -> Result<(Arc<Generation>, mpsc::UnboundedReceiver<GenEvent>), String> {
        let (ours, theirs) = Channel::pair().map_err(|err| format!("socketpair: {err}"))?;
        let child_fd = theirs.as_raw_fd();
        let mut command = tokio::process::Command::new(binary);
        command
            .arg("serve")
            .env("SPEEDYSPEC_CHANNEL_FD", "3")
            .kill_on_drop(false);
        // SAFETY: only async-signal-safe `dup2` runs between fork and exec; it places the channel on fd 3 (and
        // clears close-on-exec on the copy).
        unsafe {
            command.as_std_mut().pre_exec(move || {
                if libc::dup2(child_fd, 3) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command
            .spawn()
            .map_err(|err| format!("starting {}: {err}", binary.display()))?;
        drop(theirs);
        let pid = child.id().unwrap_or(0);
        let channel = Arc::new(ours.into_async().map_err(|err| format!("channel: {err}"))?);

        let infos: Vec<SocketInfo> = self
            .sockets
            .list
            .iter()
            .map(|(info, _)| info.clone())
            .collect();
        let fds: Vec<_> = self.sockets.list.iter().map(|(_, fd)| fd.as_fd()).collect();
        let role = if self.config.mode == Mode::Managed {
            GenerationRole::Managed
        } else {
            GenerationRole::Immutable
        };
        channel
            .send(
                &ToGeneration::Hello {
                    protocol: PROTOCOL,
                    config: absolute(&self.config_path),
                    sockets: infos,
                    role,
                },
                &fds,
            )
            .await
            .map_err(|err| format!("handing sockets to generation {pid}: {err}"))?;

        tracing::info!(event = "generation_started", pid, binary = %binary.display());
        Ok(attach(channel, pid, ChildProc::Spawned(child)))
    }

    /// `reload`: re-reads the config and hands off to a new generation of the confirmed binary. Refused in immutable
    /// mode, and for listener changes, which need the keeper to rebind (a full restart).
    async fn reload(&self) -> Result<String, String> {
        if self.config.mode != Mode::Managed {
            return Err(control::IMMUTABLE_REFUSAL.to_string());
        }
        let next = Config::load(&self.config_path).map_err(|err| err.to_string())?;
        let shape = |config: &Config| -> Vec<(std::net::SocketAddr, bool, bool)> {
            config
                .listeners
                .iter()
                .map(|l| (l.address, l.tls.is_some(), l.http3))
                .collect()
        };
        if shape(&next) != shape(&self.config) {
            return Err(format!(
                "listener changes need a full restart (configured {:?}, running {:?})",
                shape(&next),
                shape(&self.config)
            ));
        }
        let (tx, rx) = oneshot::channel();
        let commands = self.commands.get().ok_or("the keeper is still starting")?;
        commands
            .send(KeeperCommand::Reload { reply: tx })
            .map_err(|_| "the keeper is stopping".to_string())?;
        rx.await
            .map_err(|_| "the keeper stopped during the reload".to_string())?
    }

    /// The active generation, waiting up to `ready_timeout` for one (at start-up, or during a crash restart).
    async fn wait_for_active(&self) -> Result<Arc<Generation>, String> {
        let deadline = tokio::time::Instant::now() + self.config.handoff.ready_timeout;
        loop {
            let notified = self.activated.notified();
            if let Some(generation) = self.active.lock().await.clone() {
                return Ok(generation);
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return Err("no generation is serving; see the server's log".to_string());
            }
        }
    }

    /// Sends one control line to `generation` and waits for its raw answer.
    pub(crate) async fn forward_to(
        &self,
        generation: &Generation,
        line: &str,
    ) -> Result<String, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        generation
            .replies
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, tx);
        if generation.closed.load(Ordering::SeqCst) {
            generation
                .replies
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&id);
            return Err("the generation exited before answering".to_string());
        }
        generation
            .send(&ToGeneration::Control(format!("{id}\t{line}")))
            .await;
        rx.await
            .map_err(|_| "the generation exited before answering".to_string())
    }

    /// Sends one control line to the active generation. A reply asking for a server upgrade is carried out here, and
    /// the client gets the upgrade's result.
    async fn forward(&self, line: &str) -> Result<String, String> {
        let generation = self.wait_for_active().await?;
        let reply = self.forward_to(&generation, line).await?;
        if let Ok(value) = serde_json::from_str::<Value>(&reply)
            && let Some(upgrade) = value.get("Ok").and_then(|ok| ok.get("upgrade"))
        {
            let version: Version = upgrade["version"]
                .as_str()
                .and_then(|v| v.parse().ok())
                .ok_or("the generation asked for an upgrade without a version")?;
            let binary = PathBuf::from(
                upgrade["binary"]
                    .as_str()
                    .ok_or("the upgrade has no binary")?,
            );
            let (tx, rx) = oneshot::channel();
            let commands = self.commands.get().ok_or("the keeper is still starting")?;
            commands
                .send(KeeperCommand::Upgrade {
                    version,
                    binary,
                    reply: tx,
                })
                .map_err(|_| "the keeper is stopping".to_string())?;
            let result = rx
                .await
                .map_err(|_| "the keeper stopped during the upgrade".to_string())?;
            let response = match result {
                Ok(message) => control::ControlResponse::Ok(Value::String(message)),
                Err(message) => control::ControlResponse::Error(message),
            };
            return Ok(serde_json::to_string(&response).expect("serializes"));
        }
        Ok(self.with_keeper_status(line, reply, &generation))
    }

    /// Adds the keeper's view (generation PID, version, trial) to a `Status` answer.
    fn with_keeper_status(&self, line: &str, reply: String, generation: &Generation) -> String {
        let is_status = serde_json::from_str::<control::ControlRequest>(line)
            .is_ok_and(|r| r == control::ControlRequest::Status);
        if !is_status {
            return reply;
        }
        let Ok(mut value) = serde_json::from_str::<Value>(&reply) else {
            return reply;
        };
        if let Some(ok) = value.get_mut("Ok").and_then(Value::as_object_mut) {
            let version = generation
                .version
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            ok.insert(
                "keeper".to_string(),
                serde_json::json!({
                    "version": own_version().to_string(),
                    "generation": {
                        "pid": generation.pid,
                        "version": version.map(|v| v.to_string()),
                        "on_trial": generation.on_trial.load(Ordering::SeqCst),
                    }
                }),
            );
        }
        value.to_string()
    }
}

struct KeeperControl(Arc<Keeper>);

impl LineHandler for KeeperControl {
    fn handle_line(
        &self,
        line: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send + '_>> {
        Box::pin(async move {
            if serde_json::from_str::<control::ControlRequest>(&line)
                .is_ok_and(|r| r == control::ControlRequest::Reload)
            {
                let response = match self.0.reload().await {
                    Ok(message) => control::ControlResponse::Ok(Value::String(message)),
                    Err(message) => control::ControlResponse::Error(message),
                };
                return serde_json::to_string(&response).expect("serializes");
            }
            match self.0.forward(&line).await {
                Ok(reply) => reply,
                Err(message) => serde_json::to_string(&control::ControlResponse::Error(message))
                    .expect("serializes"),
            }
        })
    }
}

/// Takes `<run_dir>/server.lock` for the life of the process.
pub(crate) fn hold_server_lock(
    run_dir: &Path,
    create_dir: bool,
) -> Result<std::os::fd::RawFd, String> {
    if create_dir {
        std::fs::create_dir_all(run_dir)
            .map_err(|err| format!("creating {}: {err}", run_dir.display()))?;
    }
    let path = run_dir.join("server.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|err| format!("opening {}: {err}", path.display()))?;
    match file.try_lock() {
        Ok(()) => {
            // Held until the process exits.
            let fd = file.as_raw_fd();
            Box::leak(Box::new(file));
            Ok(fd)
        }
        Err(std::fs::TryLockError::WouldBlock) => Err(format!(
            "another speedyspec-server is already running with run_dir {}",
            run_dir.display()
        )),
        Err(std::fs::TryLockError::Error(err)) => Err(format!("locking {}: {err}", path.display())),
    }
}

/// Runs the keeper until `SIGTERM`/`SIGINT`.
pub fn run(config_path: &Path) -> Result<(), String> {
    let runtime =
        tokio::runtime::Runtime::new().map_err(|err| format!("starting the runtime: {err}"))?;
    runtime.block_on(run_async(config_path))
}

async fn run_async(config_path: &Path) -> Result<(), String> {
    use tokio::signal::unix::{SignalKind, signal};

    let config = Config::load(config_path).map_err(|err| err.to_string())?;
    let managed = config.mode == Mode::Managed;
    let term = signal(SignalKind::terminate()).map_err(|err| err.to_string())?;
    let interrupt = signal(SignalKind::interrupt()).map_err(|err| err.to_string())?;
    let hangup = signal(SignalKind::hangup()).map_err(|err| err.to_string())?;

    if let Ok(saved) = std::env::var(KEEPER_STATE) {
        let (keeper, current, confirmed, carry) = resume(config, config_path, &saved).await?;
        return supervise(keeper, current, confirmed, carry, term, interrupt, hangup).await;
    }
    let mut carry = Carry::default();
    match hold_server_lock(&config.run_dir, managed) {
        Ok(fd) => carry.lock = Some(fd),
        Err(err) if managed || err.contains("already running") => return Err(err),
        Err(err) => tracing::warn!(event = "lock_unavailable", error = %err),
    }
    let (tcp, udp) = bind(&config).map_err(|err| err.to_string())?;
    let mut list = Vec::new();
    for (bound, listener) in tcp.into_iter().zip(&config.listeners) {
        let info = SocketInfo {
            kind: SocketKind::Tcp,
            address: bound.listener.local_addr().map_err(|err| err.to_string())?,
            tls: bound.tls.as_ref().map(|t| (t.cert.clone(), t.key.clone())),
            http3: listener.http3,
        };
        list.push((info, OwnedFd::from(bound.listener)));
    }
    for bound in udp {
        let info = SocketInfo {
            kind: SocketKind::Udp,
            address: bound.socket.local_addr().map_err(|err| err.to_string())?,
            tls: Some((bound.tls.cert.clone(), bound.tls.key.clone())),
            http3: true,
        };
        list.push((info, OwnedFd::from(bound.socket)));
    }

    let binary = if managed {
        crate::managed::confirmed_binary(&config)?
    } else {
        std::env::current_exe().map_err(|err| err.to_string())?
    };
    let keeper = Arc::new(Keeper {
        config_path: config_path.to_path_buf(),
        config,
        sockets: Sockets { list },
        active: Mutex::new(None),
        activated: tokio::sync::Notify::new(),
        commands: std::sync::OnceLock::new(),
        next_id: AtomicU64::new(1),
    });
    let socket = control::socket_path(&keeper.config.run_dir);
    match control::serve(&socket, Arc::new(KeeperControl(keeper.clone())), managed) {
        Ok(fd) => carry.keeper_sock = Some(fd),
        Err(err) => {
            if err.kind() == std::io::ErrorKind::AddrInUse || managed {
                return Err(err.to_string());
            }
            tracing::warn!(event = "control_unavailable", path = %socket.display(), error = %err);
        }
    }
    let confirmed = binary;

    // The first generation: one that never becomes ready is a start-up error, not a crash to restart.
    let current = match keeper.start_ready(&confirmed).await {
        Ok(live) => live,
        Err(why) => {
            return Err(format!(
                "the server did not start ({why}); see the errors above"
            ));
        }
    };
    keeper.activate(&current).await;
    crate::notify::ready();
    supervise(keeper, current, confirmed, carry, term, interrupt, hangup).await
}

/// What a re-exec must carry besides the listening sockets.
#[derive(Default)]
pub(crate) struct Carry {
    lock: Option<std::os::fd::RawFd>,
    keeper_sock: Option<OwnedFd>,
}

/// The environment variable a re-executing keeper passes its state in.
const KEEPER_STATE: &str = "SPEEDYSPEC_KEEPER_STATE";

/// A keeper's state across its own `execve`: fd numbers, the active generation, and the confirmed binary.
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedKeeper {
    sockets: Vec<(SocketInfo, i32)>,
    keeper_sock: Option<i32>,
    lock: Option<i32>,
    generation: (u32, Option<Version>, i32),
    confirmed: PathBuf,
}

/// Supervises generations until the keeper is told to stop: crash restarts, upgrades and their trials, reloads, and
/// re-executing itself after an upgrade to a newer binary.
async fn supervise(
    keeper: Arc<Keeper>,
    mut current: Live,
    mut confirmed: PathBuf,
    carry: Carry,
    mut term: tokio::signal::unix::Signal,
    mut interrupt: tokio::signal::unix::Signal,
    mut hangup: tokio::signal::unix::Signal,
) -> Result<(), String> {
    let (commands, mut command_rx) = mpsc::unbounded_channel();
    let _ = keeper.commands.set(commands);
    let mut old: Option<Live> = None;
    // (version, binary, ends at, whether it is a reload)
    let mut trial: Option<(Version, PathBuf, tokio::time::Instant, bool)> = None;
    let mut reexec_pending = false;
    // The confirmed trial's previous generation, draining after its Shutdown; reaped before any re-exec, since a new
    // keeper image could not reap it.
    let mut retiring: Option<tokio::task::JoinHandle<()>> = None;
    let mut backoff = BACKOFF_START;
    let mut started = Instant::now();
    loop {
        if reexec_pending && old.is_none() && trial.is_none() && retiring.is_none() {
            reexec_pending = false;
            let err = keeper.reexec(&confirmed, &current, &carry);
            tracing::error!(event = "keeper_reexec_failed", error = %err);
        }
        let trial_end = trial.as_ref().map(|(_, _, at, _)| *at);
        tokio::select! {
            status = exited(&current.generation) => {
                tracing::warn!(event = "generation_exited", pid = current.generation.pid, status = ?status);
                *keeper.active.lock().await = None;
                if let Some((version, _, _, reload)) = trial.take() {
                    tracing::error!(event = "trial_failed", version = %version);
                    match old.take() {
                        Some(previous) => {
                            let failed = (!reload).then_some(version);
                            previous.generation.send(&ToGeneration::Resume { failed }).await;
                            current = previous;
                        }
                        None => {
                            current = keeper.restart(&confirmed, &mut backoff, &mut started).await?;
                        }
                    }
                    keeper.activate(&current).await;
                } else {
                    *keeper.active.lock().await = None;
                    if started.elapsed() >= BACKOFF_MAX {
                        backoff = BACKOFF_START;
                    }
                    current = keeper.restart(&confirmed, &mut backoff, &mut started).await?;
                    keeper.activate(&current).await;
                }
            }
            _ = async { retiring.as_mut().expect("guarded").await }, if retiring.is_some() => {
                retiring = None;
            }
            status = maybe_exited(old.as_ref()) => {
                tracing::info!(event = "old_generation_exited", status = ?status);
                old = None;
            }
            () = sleep_until(trial_end) => {
                let (version, binary, _, reload) = trial.take().expect("only armed during a trial");
                let line = serde_json::to_string(&control::ControlRequest::ConfirmServer { version: version.clone() })
                    .expect("serializes");
                if let Err(err) = keeper.forward_to(&current.generation, &line).await {
                    tracing::error!(event = "confirm_failed", version = %version, error = %err);
                }
                current.generation.on_trial.store(false, Ordering::SeqCst);
                confirmed = binary;
                if !reload && version > own_version() {
                    reexec_pending = true;
                }
                if let Some(previous) = old.take() {
                    previous.generation.send(&ToGeneration::Shutdown).await;
                    retiring = Some(tokio::spawn(async move {
                        let _ = exited(&previous.generation).await;
                    }));
                }
                tracing::info!(event = "trial_confirmed", version = %version);
            }
            Some(command) = command_rx.recv() => {
                let (version, binary, reload, reply) = match command {
                    KeeperCommand::Upgrade { version, binary, reply } => (version, binary, false, reply),
                    KeeperCommand::Reload { reply } => {
                        let version = current.generation.version.lock().unwrap_or_else(|p| p.into_inner()).clone();
                        let Some(version) = version else {
                            let _ = reply.send(Err("the current generation has not reported its version".to_string()));
                            continue;
                        };
                        (version, confirmed.clone(), true, reply)
                    }
                };
                let failed = (!reload).then(|| version.clone());
                if let Some((on_trial, _, _, _)) = &trial {
                    let _ = reply.send(Err(format!("an upgrade to server {on_trial} is on trial; wait for it to be confirmed or to fail")));
                    continue;
                }
                if old.is_some() {
                    let _ = reply.send(Err("the previous generation is still draining; try again shortly".to_string()));
                    continue;
                }
                if !reload {
                    use speedyspec_server_domain::BinaryInspector;
                    let needs = crate::binary::ProcessInspector.manifest(&binary).map(|m| m.protocol.unwrap_or(1));
                    if let Ok(needs) = needs
                        && needs > PROTOCOL
                    {
                        current.generation.send(&ToGeneration::Resume { failed: failed.clone() }).await;
                        let _ = reply.send(Err(format!(
                            "binary {version} needs keeper protocol {needs}; this keeper speaks protocol {PROTOCOL}"
                        )));
                        continue;
                    }
                }
                match keeper.start_ready(&binary).await {
                    Err(why) => {
                        tracing::error!(event = "upgrade_failed", version = %version, error = %why);
                        current.generation.send(&ToGeneration::Resume { failed: failed.clone() }).await;
                        let _ = reply.send(Err(format!("server {version} did not become ready: {why}")));
                    }
                    Ok(next) => {
                        current.generation.send(&ToGeneration::Drain).await;
                        if !current.wait_for(|event| matches!(event, GenEvent::UdpReleased), Duration::from_secs(15)).await {
                            tracing::warn!(event = "udp_release_slow", pid = current.generation.pid);
                        }
                        next.generation.send(&ToGeneration::StartUdp).await;
                        next.generation.on_trial.store(true, Ordering::SeqCst);
                        let trial_period = keeper.config.handoff.trial_period;
                        old = Some(std::mem::replace(&mut current, next));
                        keeper.activate(&current).await;
                        trial = Some((version.clone(), binary, tokio::time::Instant::now() + trial_period, reload));
                        tracing::info!(event = "upgrade_on_trial", version = %version, pid = current.generation.pid, reload);
                        let message = if reload {
                            format!("reloaded (new generation on trial for {})", humantime::format_duration(trial_period))
                        } else {
                            format!("switched server {version} (on trial for {})", humantime::format_duration(trial_period))
                        };
                        let _ = reply.send(Ok(message));
                    }
                }
            }
            event = current.events.recv() => {
                if let Some(event) = event {
                    tracing::debug!(event = "generation_event", message = ?event);
                }
            }
            _ = hangup.recv() => current.generation.send(&ToGeneration::ReloadCerts).await,
            _ = term.recv() => return keeper.stop(current, old).await,
            _ = interrupt.recv() => return keeper.stop(current, old).await,
        }
    }
}

/// A generation and its event stream.
pub(crate) struct Live {
    pub(crate) generation: Arc<Generation>,
    events: mpsc::UnboundedReceiver<GenEvent>,
}

impl Live {
    /// Waits for an event matching `wanted`, up to `timeout`.
    async fn wait_for(&mut self, wanted: impl Fn(&GenEvent) -> bool, timeout: Duration) -> bool {
        tokio::time::timeout(timeout, async {
            while let Some(event) = self.events.recv().await {
                if wanted(&event) {
                    return true;
                }
                if matches!(event, GenEvent::Closed) {
                    return false;
                }
            }
            false
        })
        .await
        .unwrap_or(false)
    }
}

/// What the control handler asks the supervisor to do.
pub(crate) enum KeeperCommand {
    Upgrade {
        version: Version,
        binary: PathBuf,
        reply: oneshot::Sender<Result<String, String>>,
    },
    Reload {
        reply: oneshot::Sender<Result<String, String>>,
    },
}

/// Clears close-on-exec, so `fd` survives the keeper's `execve`.
fn inherit(fd: std::os::fd::RawFd) -> std::io::Result<()> {
    // SAFETY: the fd is open and owned by this process for as long as the keeper runs.
    let fd = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
    rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::empty()).map_err(std::io::Error::from)
}

/// Sets close-on-exec again on an fd adopted from a previous keeper, so generations do not inherit it.
fn keep_private(fd: std::os::fd::RawFd) {
    // SAFETY: as above.
    let fd = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
    let _ = rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::CLOEXEC);
}

impl Keeper {
    /// Replaces this process with `binary` (`run --config …`), handing it the sockets, the lock, `keeper.sock` and the
    /// active generation. Returns only on failure.
    fn reexec(&self, binary: &Path, current: &Live, carry: &Carry) -> String {
        use std::os::unix::process::CommandExt;

        let channel_fd = current.generation.channel.as_raw_fd();
        let mut fds: Vec<i32> = self
            .sockets
            .list
            .iter()
            .map(|(_, fd)| fd.as_raw_fd())
            .collect();
        fds.push(channel_fd);
        fds.extend(carry.keeper_sock.as_ref().map(|fd| fd.as_raw_fd()));
        fds.extend(carry.lock);
        for fd in &fds {
            if let Err(err) = inherit(*fd) {
                return format!("keeping fd {fd} open across exec: {err}");
            }
        }
        let saved = SavedKeeper {
            sockets: self
                .sockets
                .list
                .iter()
                .map(|(info, fd)| (info.clone(), fd.as_raw_fd()))
                .collect(),
            keeper_sock: carry.keeper_sock.as_ref().map(|fd| fd.as_raw_fd()),
            lock: carry.lock,
            generation: (
                current.generation.pid,
                current
                    .generation
                    .version
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone(),
                channel_fd,
            ),
            confirmed: binary.to_path_buf(),
        };
        tracing::info!(event = "keeper_reexecuting", binary = %binary.display());
        let err = std::process::Command::new(binary)
            .arg("run")
            .arg("--config")
            .arg(&self.config_path)
            .env(
                KEEPER_STATE,
                serde_json::to_string(&saved).expect("serializes"),
            )
            .exec();
        for fd in &fds {
            keep_private(*fd);
        }
        format!("exec {}: {err}", binary.display())
    }
}

/// Starts a keeper from the state a previous keeper passed across its `execve`.
async fn resume(
    config: Config,
    config_path: &Path,
    saved: &str,
) -> Result<(Arc<Keeper>, Live, PathBuf, Carry), String> {
    use std::os::fd::FromRawFd;

    let saved: SavedKeeper =
        serde_json::from_str(saved).map_err(|err| format!("{KEEPER_STATE}: {err}"))?;
    let mut list = Vec::new();
    for (info, fd) in saved.sockets {
        keep_private(fd);
        // SAFETY: the previous keeper left this fd open for us, and nothing else owns it.
        list.push((info, unsafe { OwnedFd::from_raw_fd(fd) }));
    }
    let keeper = Arc::new(Keeper {
        config_path: config_path.to_path_buf(),
        config,
        sockets: Sockets { list },
        active: Mutex::new(None),
        activated: tokio::sync::Notify::new(),
        commands: std::sync::OnceLock::new(),
        next_id: AtomicU64::new(1),
    });
    let mut carry = Carry {
        lock: saved.lock,
        keeper_sock: None,
    };
    if let Some(fd) = saved.lock {
        keep_private(fd);
    }
    if let Some(fd) = saved.keeper_sock {
        keep_private(fd);
        // SAFETY: as above.
        let listener = unsafe { std::os::unix::net::UnixListener::from_raw_fd(fd) };
        let copy = listener.try_clone().map_err(|err| err.to_string())?;
        control::serve_listener(listener, Arc::new(KeeperControl(keeper.clone())))
            .map_err(|err| err.to_string())?;
        carry.keeper_sock = Some(OwnedFd::from(copy));
    }
    let (pid, version, channel_fd) = saved.generation;
    keep_private(channel_fd);
    // SAFETY: as above.
    let channel = Channel::from_fd(unsafe { OwnedFd::from_raw_fd(channel_fd) })
        .and_then(Channel::into_async)
        .map_err(|err| format!("adopting generation {pid}: {err}"))?;
    let (generation, events) = attach(Arc::new(channel), pid, ChildProc::adopt(pid));
    *generation.version.lock().unwrap_or_else(|p| p.into_inner()) = version;
    let current = Live { generation, events };
    keeper.activate(&current).await;
    tracing::info!(event = "keeper_reexecuted", pid = std::process::id(), version = %own_version(), generation = pid);
    Ok((keeper, current, saved.confirmed, carry))
}

async fn exited(generation: &Generation) -> Option<std::process::ExitStatus> {
    generation.child.lock().await.wait().await
}

/// A generation process: one this keeper spawned, or one it adopted across its own re-exec.
pub(crate) enum ChildProc {
    Spawned(tokio::process::Child),
    /// Waited on by a thread that calls `waitpid` once and publishes the status.
    Adopted(tokio::sync::watch::Receiver<Option<i32>>),
}

impl ChildProc {
    fn adopt(pid: u32) -> ChildProc {
        let (tx, rx) = tokio::sync::watch::channel(None);
        std::thread::spawn(move || {
            let mut status = 0;
            // SAFETY: waiting for a child of this process, which survived the keeper's exec.
            unsafe { libc::waitpid(pid as libc::pid_t, &mut status, 0) };
            let _ = tx.send(Some(status));
        });
        ChildProc::Adopted(rx)
    }

    async fn wait(&mut self) -> Option<std::process::ExitStatus> {
        use std::os::unix::process::ExitStatusExt;
        match self {
            ChildProc::Spawned(child) => child.wait().await.ok(),
            ChildProc::Adopted(rx) => {
                let status = *rx.wait_for(Option::is_some).await.ok()?;
                status.map(std::process::ExitStatus::from_raw)
            }
        }
    }
}

/// Wraps a generation's channel: starts its reader, which routes control replies and reports events.
fn attach(
    channel: Arc<AsyncChannel>,
    pid: u32,
    child: ChildProc,
) -> (Arc<Generation>, mpsc::UnboundedReceiver<GenEvent>) {
    let replies: Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<String>>>> = Arc::default();
    let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (events, receiver) = mpsc::unbounded_channel();
    {
        let (channel, replies, closed) = (channel.clone(), replies.clone(), closed.clone());
        tokio::spawn(async move {
            loop {
                let event = match channel.recv::<ToKeeper>().await {
                    Ok(Some((ToKeeper::Ready { version, .. }, _))) => GenEvent::Ready(version),
                    Ok(Some((ToKeeper::UdpReleased, _))) => GenEvent::UdpReleased,
                    Ok(Some((ToKeeper::Upgrade { version, binary }, _))) => {
                        GenEvent::Upgrade { version, binary }
                    }
                    Ok(Some((ToKeeper::Reload, _))) => GenEvent::Reload,
                    Ok(Some((ToKeeper::Exiting, _))) => GenEvent::Exiting,
                    Ok(Some((ToKeeper::ControlReply(line), _))) => {
                        if let Some((id, reply)) = line.split_once('\t')
                            && let Ok(id) = id.parse::<u64>()
                            && let Some(waiting) = replies
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .remove(&id)
                        {
                            let _ = waiting.send(reply.to_string());
                        }
                        continue;
                    }
                    Ok(None) | Err(_) => {
                        closed.store(true, Ordering::SeqCst);
                        // Dropping the waiting senders fails every request still in flight.
                        replies.lock().unwrap_or_else(|p| p.into_inner()).clear();
                        let _ = events.send(GenEvent::Closed);
                        return;
                    }
                };
                if events.send(event).is_err() {
                    return;
                }
            }
        });
    }
    let generation = Arc::new(Generation {
        pid,
        version: std::sync::Mutex::new(None),
        on_trial: std::sync::atomic::AtomicBool::new(false),
        closed,
        channel,
        replies,
        child: Mutex::new(child),
    });
    (generation, receiver)
}

async fn maybe_exited(live: Option<&Live>) -> Option<std::process::ExitStatus> {
    match live {
        Some(live) => exited(&live.generation).await,
        None => std::future::pending().await,
    }
}

async fn sleep_until(at: Option<tokio::time::Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

fn kill(pid: u32) {
    // SAFETY: sending a signal to a child PID this keeper started.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
}

impl Keeper {
    /// Starts a generation of `binary` and waits for it to be ready; kills it (after a moment to say why) if not.
    async fn start_ready(&self, binary: &Path) -> Result<Live, String> {
        let (generation, events) = self.spawn(binary).await?;
        let mut live = Live { generation, events };
        let ready = tokio::time::timeout(self.config.handoff.ready_timeout, async {
            while let Some(event) = live.events.recv().await {
                match event {
                    GenEvent::Ready(version) => return Ok(version),
                    GenEvent::Closed => return Err("it exited before becoming ready".to_string()),
                    _ => {}
                }
            }
            Err("it exited before becoming ready".to_string())
        })
        .await
        .unwrap_or_else(|_| {
            Err(format!(
                "it was not ready within {}",
                humantime::format_duration(self.config.handoff.ready_timeout)
            ))
        });
        match ready {
            Ok(version) => {
                *live
                    .generation
                    .version
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = Some(version);
                Ok(live)
            }
            Err(why) => {
                tracing::error!(event = "generation_not_ready", pid = live.generation.pid, error = %why);
                if tokio::time::timeout(Duration::from_secs(2), exited(&live.generation))
                    .await
                    .is_err()
                {
                    kill(live.generation.pid);
                    let _ = exited(&live.generation).await;
                }
                Err(why)
            }
        }
    }

    /// Makes `live` the generation control requests go to, and lets it read UDP.
    async fn activate(&self, live: &Live) {
        live.generation.send(&ToGeneration::StartUdp).await;
        *self.active.lock().await = Some(live.generation.clone());
        self.activated.notify_waiters();
        let version = live
            .generation
            .version
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        tracing::info!(event = "generation_active", pid = live.generation.pid, version = ?version.map(|v| v.to_string()));
    }

    /// A crash restart from `binary`, after the back-off, retrying until a generation is ready.
    async fn restart(
        &self,
        binary: &Path,
        backoff: &mut Duration,
        started: &mut Instant,
    ) -> Result<Live, String> {
        loop {
            tracing::info!(event = "generation_restarting", after = ?*backoff);
            tokio::time::sleep(*backoff).await;
            *backoff = (*backoff * 2).min(BACKOFF_MAX);
            *started = Instant::now();
            match self.start_ready(binary).await {
                Ok(live) => return Ok(live),
                Err(why) => tracing::error!(event = "restart_failed", error = %why),
            }
        }
    }

    /// Drains every generation and exits.
    async fn stop(&self, current: Live, old: Option<Live>) -> Result<(), String> {
        tracing::info!(event = "keeper_stopping", pid = current.generation.pid);
        let deadline = self.config.handoff.drain_timeout + Duration::from_secs(5);
        let mut all = vec![current];
        all.extend(old);
        for live in &all {
            live.generation.send(&ToGeneration::Shutdown).await;
        }
        for live in &all {
            if tokio::time::timeout(deadline, exited(&live.generation))
                .await
                .is_err()
            {
                kill(live.generation.pid);
                let _ = exited(&live.generation).await;
            }
        }
        Ok(())
    }
}
