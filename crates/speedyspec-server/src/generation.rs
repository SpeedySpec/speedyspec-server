//! The `serve` role: one generation, started only by a keeper, serving on the sockets the keeper passed it.

use std::net::{TcpListener, UdpSocket};
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use speedyspec_server_handoff::{
    AsyncChannel, Channel, PROTOCOL, SocketKind, ToGeneration, ToKeeper,
};
use speedyspec_server_presentation::build_router;

use crate::binary::{exit_after_ready, never_ready, own_version};
use crate::boot::{immutable_manifests, prepare};
use crate::config::{Config, Mode};
use crate::control::{self, ControlBackend, ImmutableBackend, ManagedBackend};
use crate::listeners::{BoundTcp, BoundUdp, TlsFiles};
use crate::managed;
use crate::serving::Serving;

/// How long a generation whose keeper vanished keeps serving open connections before it exits.
const ORPHAN_DRAIN: Duration = Duration::from_secs(30);

/// Runs a generation on the channel at fd `SPEEDYSPEC_CHANNEL_FD` (3).
pub fn run() -> Result<(), String> {
    let fd: i32 = std::env::var("SPEEDYSPEC_CHANNEL_FD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    // SAFETY: the keeper placed one end of the channel socketpair on this fd before exec, and nothing else owns it.
    let channel = Channel::from_fd(unsafe { OwnedFd::from_raw_fd(fd) })
        .map_err(|err| format!("channel: {err}"))?;
    let runtime =
        tokio::runtime::Runtime::new().map_err(|err| format!("starting the runtime: {err}"))?;
    runtime.block_on(serve(channel))
}

async fn serve(channel: Channel) -> Result<(), String> {
    let channel = Arc::new(
        channel
            .into_async()
            .map_err(|err| format!("channel: {err}"))?,
    );
    let Some((hello, fds)) = channel
        .recv::<ToGeneration>()
        .await
        .map_err(|err| format!("channel: {err}"))?
    else {
        return Err("the keeper closed the channel before saying hello".to_string());
    };
    let ToGeneration::Hello {
        protocol,
        config,
        sockets,
        ..
    } = hello
    else {
        return Err("the keeper's first message was not Hello".to_string());
    };
    if protocol != PROTOCOL {
        return Err(format!(
            "the keeper speaks protocol {protocol}; this generation speaks protocol {PROTOCOL}"
        ));
    }
    if fds.len() != sockets.len() {
        return Err(format!(
            "the keeper named {} sockets but passed {} fds",
            sockets.len(),
            fds.len()
        ));
    }
    let config = Config::load(&config).map_err(|err| err.to_string())?;
    let mut tcp = Vec::new();
    let mut udp = Vec::new();
    for (info, fd) in sockets.into_iter().zip(fds) {
        let tls = info.tls.map(|(cert, key)| TlsFiles { cert, key });
        match info.kind {
            SocketKind::Tcp => tcp.push(BoundTcp {
                listener: TcpListener::from(fd),
                tls,
            }),
            SocketKind::Udp => {
                let tls = tls
                    .ok_or_else(|| format!("UDP socket {} came without TLS files", info.address))?;
                udp.push(BoundUdp {
                    socket: UdpSocket::from(fd),
                    tls,
                });
            }
        }
    }

    let state = prepare(&config).map_err(|err| err.to_string())?;
    let live = state.apps.clone();
    let version = own_version();
    let backend: Arc<dyn ControlBackend> = if config.mode == Mode::Managed {
        let (settings, apps) = (config.clone(), live.clone());
        let applied = version.clone();
        let manager = tokio::task::spawn_blocking(move || {
            let manager = managed::start(&settings, apps)?;
            manager
                .apply_pending_group(&applied)
                .map_err(|err| err.to_string())?;
            Ok::<_, String>(manager)
        })
        .await
        .map_err(|err| format!("starting releases: {err}"))??;
        managed::spawn_auto_update(manager.clone(), managed::auto_apps(&config));
        managed::spawn_fallback_sweep(live);
        let apps = config
            .apps
            .iter()
            .map(|(id, app)| (id.clone(), app.kind))
            .collect();
        Arc::new(ManagedBackend {
            manager,
            apps,
            server_version: version.clone(),
        })
    } else {
        let manifests = immutable_manifests(&config).map_err(|err| err.to_string())?;
        Arc::new(ImmutableBackend {
            manifests,
            server_version: version.clone(),
        })
    };
    let serving = Serving::start(tcp, udp, build_router(Arc::new(state)), false)
        .await
        .map_err(|err| format!("{err:#}"))?;

    if !never_ready() {
        channel
            .send(
                &ToKeeper::Ready {
                    version: version.clone(),
                    protocol: PROTOCOL,
                },
                &[],
            )
            .await
            .map_err(|err| format!("channel: {err}"))?;
        tracing::info!(event = "generation_ready", pid = std::process::id(), version = %version);
    }
    if let Some(after) = exit_after_ready() {
        tokio::spawn(async move {
            tokio::time::sleep(after).await;
            tracing::warn!(event = "test_hook_exit");
            std::process::exit(3);
        });
    }

    let signals = signals().map_err(|err| format!("installing signal handlers: {err}"))?;
    tokio::pin!(signals);
    loop {
        tokio::select! {
            () = &mut signals => {
                return shutdown(&serving, &channel, config.handoff.drain_timeout).await;
            }
            message = channel.recv::<ToGeneration>() => {
                match message.map_err(|err| format!("channel: {err}"))? {
                    None => {
                        tracing::warn!(event = "keeper_gone", "the keeper's channel closed; draining and exiting");
                        serving.stop_accepting().await;
                        serving.wait_idle(ORPHAN_DRAIN).await;
                        return Ok(());
                    }
                    Some((ToGeneration::StartUdp, _)) => {
                        serving.start_udp().await.map_err(|err| format!("{err:#}"))?;
                    }
                    Some((ToGeneration::ReloadCerts, _)) => {
                        if let Err(err) = serving.reload_certs().await {
                            tracing::error!(event = "certificate_reload_failed", error = %format!("{err:#}"));
                        }
                    }
                    Some((ToGeneration::Shutdown, _)) => {
                        return shutdown(&serving, &channel, config.handoff.drain_timeout).await;
                    }
                    Some((ToGeneration::Control(line), _)) => {
                        let (backend, channel) = (backend.clone(), channel.clone());
                        tokio::spawn(async move { reply(&*channel, backend, line).await });
                    }
                    Some((other, _)) => tracing::info!(event = "handoff_message_ignored", message = ?other),
                }
            }
        }
    }
}

/// Answers one `Control` message: the line is `<id>\t<request>`, and the reply carries the same id.
async fn reply(channel: &AsyncChannel, backend: Arc<dyn ControlBackend>, line: String) {
    let (id, request) = line.split_once('\t').unwrap_or(("0", line.as_str()));
    let (id, request) = (id.to_string(), request.to_string());
    let answer = tokio::task::spawn_blocking(move || control::answer(&*backend, &request))
        .await
        .unwrap_or_else(|err| format!("{{\"Error\":\"the request failed: {err}\"}}"));
    if let Err(err) = channel
        .send(&ToKeeper::ControlReply(format!("{id}\t{answer}")), &[])
        .await
    {
        tracing::warn!(event = "control_reply_failed", error = %err);
    }
}

/// Stops accepting, lets open connections finish (up to `drain`), tells the keeper, and returns.
async fn shutdown(
    serving: &Serving,
    channel: &AsyncChannel,
    drain: Duration,
) -> Result<(), String> {
    tracing::info!(event = "generation_shutting_down", pid = std::process::id());
    serving.stop_accepting().await;
    serving.wait_idle(drain).await;
    let _ = channel.send(&ToKeeper::Exiting, &[]).await;
    Ok(())
}

/// Resolves on `SIGTERM` or `SIGINT` (a Ctrl-C in a terminal reaches the whole process group).
fn signals() -> std::io::Result<impl std::future::Future<Output = ()>> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut term = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    Ok(async move {
        tokio::select! {
            _ = term.recv() => {}
            _ = interrupt.recv() => {}
        }
    })
}

/// The config path as the keeper passes it: absolute, so the generation's working directory does not matter.
pub fn absolute(path: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
