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
    channel: Arc<AsyncChannel>,
    replies: Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<String>>>>,
    pub(crate) child: Mutex<tokio::process::Child>,
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

        let replies: Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<String>>>> = Arc::default();
        let (events, receiver) = mpsc::unbounded_channel();
        {
            let (channel, replies) = (channel.clone(), replies.clone());
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
        tracing::info!(event = "generation_started", pid, binary = %binary.display());
        let generation = Arc::new(Generation {
            pid,
            version: std::sync::Mutex::new(None),
            on_trial: std::sync::atomic::AtomicBool::new(false),
            channel,
            replies,
            child: Mutex::new(child),
        });
        Ok((generation, receiver))
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

    /// Sends one control line to the active generation and waits for its answer.
    async fn forward(&self, line: &str) -> Result<String, String> {
        let generation = self.wait_for_active().await?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        generation
            .replies
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, tx);
        generation
            .send(&ToGeneration::Control(format!("{id}\t{line}")))
            .await;
        let reply = rx
            .await
            .map_err(|_| "the generation exited before answering".to_string())?;
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
            match self.0.forward(&line).await {
                Ok(reply) => reply,
                Err(message) => serde_json::to_string(&control::ControlResponse::Error(message))
                    .expect("serializes"),
            }
        })
    }
}

/// Takes `<run_dir>/server.lock` for the life of the process.
pub(crate) fn hold_server_lock(run_dir: &Path, create_dir: bool) -> Result<(), String> {
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
            Box::leak(Box::new(file));
            Ok(())
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
    let mut term = signal(SignalKind::terminate()).map_err(|err| err.to_string())?;
    let mut interrupt = signal(SignalKind::interrupt()).map_err(|err| err.to_string())?;
    let mut hangup = signal(SignalKind::hangup()).map_err(|err| err.to_string())?;

    match hold_server_lock(&config.run_dir, managed) {
        Ok(()) => {}
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
        next_id: AtomicU64::new(1),
    });
    let socket = control::socket_path(&keeper.config.run_dir);
    if let Err(err) = control::serve(&socket, Arc::new(KeeperControl(keeper.clone())), managed) {
        if err.kind() == std::io::ErrorKind::AddrInUse || managed {
            return Err(err.to_string());
        }
        tracing::warn!(event = "control_unavailable", path = %socket.display(), error = %err);
    }

    let mut backoff = BACKOFF_START;
    let mut notified = false;
    loop {
        let (generation, mut events) = keeper.spawn(&binary).await?;
        let started = Instant::now();
        let ready = tokio::time::timeout(keeper.config.handoff.ready_timeout, async {
            while let Some(event) = events.recv().await {
                match event {
                    GenEvent::Ready(version) => return Some(version),
                    GenEvent::Closed => return None,
                    _ => {}
                }
            }
            None
        })
        .await;
        match ready {
            Ok(Some(version)) => {
                *generation.version.lock().unwrap_or_else(|p| p.into_inner()) =
                    Some(version.clone());
                generation.send(&ToGeneration::StartUdp).await;
                *keeper.active.lock().await = Some(generation.clone());
                keeper.activated.notify_waiters();
                tracing::info!(event = "generation_active", pid = generation.pid, version = %version);
                if !notified {
                    crate::notify::ready();
                    notified = true;
                }
            }
            _ => {
                tracing::error!(event = "generation_not_ready", pid = generation.pid);
                let mut child = generation.child.lock().await;
                // A generation that closed its channel is exiting on its own; let it print why before killing it.
                if tokio::time::timeout(Duration::from_secs(2), child.wait())
                    .await
                    .is_err()
                {
                    let _ = child.start_kill();
                }
                if !notified {
                    // The very first generation never became ready: a start-up error (its reasons are already on
                    // stderr), not a crash to restart.
                    let _ = child.wait().await;
                    return Err("the server did not start; see the errors above".to_string());
                }
            }
        }

        // Supervise until the generation exits or the keeper is told to stop.
        let stop = loop {
            let mut child = generation.child.lock().await;
            tokio::select! {
                status = child.wait() => {
                    drop(child);
                    tracing::warn!(event = "generation_exited", pid = generation.pid, status = ?status.ok());
                    break false;
                }
                _ = term.recv() => break true,
                _ = interrupt.recv() => break true,
                _ = hangup.recv() => {
                    drop(child);
                    generation.send(&ToGeneration::ReloadCerts).await;
                }
                event = events.recv() => {
                    drop(child);
                    if let Some(event) = event {
                        tracing::info!(event = "generation_event", pid = generation.pid, message = ?event);
                    }
                }
            }
        };
        if stop {
            tracing::info!(event = "keeper_stopping", pid = generation.pid);
            generation.send(&ToGeneration::Shutdown).await;
            let deadline = keeper.config.handoff.drain_timeout + Duration::from_secs(5);
            let mut child = generation.child.lock().await;
            if tokio::time::timeout(deadline, child.wait()).await.is_err() {
                let _ = child.kill().await;
            }
            return Ok(());
        }
        *keeper.active.lock().await = None;
        if started.elapsed() >= BACKOFF_MAX {
            backoff = BACKOFF_START;
        }
        tracing::info!(event = "generation_restarting", after = ?backoff);
        tokio::select! {
            () = tokio::time::sleep(backoff) => {}
            _ = term.recv() => return Ok(()),
            _ = interrupt.recv() => return Ok(()),
        }
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}
