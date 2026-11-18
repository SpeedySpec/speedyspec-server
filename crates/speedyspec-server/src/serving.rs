//! Serving the router on pre-bound sockets, in a way a generation can pause, hand back and resume (see
//! `docs/handoff.md`).
//!
//! Every socket is kept as a master copy. Each start builds a fresh Salvo `Server` on a `try_clone` of it, because a
//! `Server` cannot be restarted once stopped. QUIC endpoints are released in the proven order: close with
//! [`UPGRADE_CLOSE_CODE`], stop the server, `wait_idle`, drop every handle.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use salvo::conn::quinn::{QuinnAcceptor, ServerConfig as QuinnServerConfig};
use salvo::conn::rustls::{Keycert, RustlsConfig, RustlsListener};
use salvo::conn::tcp::TcpAcceptor;
use salvo::conn::{Acceptor, Listener};
use salvo::http::header::{ALT_SVC, HeaderValue};
use salvo::http::uri::Scheme;
use salvo::server::ServerHandle;
use salvo::{Depot, FlowCtrl, Handler, Request, Response, Router, Server, Service, async_trait};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::listeners::{BoundTcp, BoundUdp, TlsFiles};

/// The QUIC application close code a draining generation sends: "server upgrading, reconnect".
pub const UPGRADE_CLOSE_CODE: u32 = 0x5353;

/// How long a releasing QUIC endpoint gets to send its close packets before it moves off the shared socket.
const CLOSE_FLUSH: Duration = Duration::from_millis(50);

/// One running Salvo server and its task.
struct Running {
    handle: ServerHandle,
    task: JoinHandle<()>,
}

struct TcpSlot {
    listener: std::net::TcpListener,
    tls: Option<TlsFiles>,
    /// Feeds new certificates to the running TLS listener.
    configs: Option<mpsc::UnboundedSender<RustlsConfig>>,
    running: Option<Running>,
}

struct UdpSlot {
    socket: std::net::UdpSocket,
    tls: TlsFiles,
    endpoint: Option<quinn::Endpoint>,
    running: Option<Running>,
}

struct Inner {
    tcp: Vec<TcpSlot>,
    udp: Vec<UdpSlot>,
    /// Servers stopped by `stop_accepting` and not yet idle.
    draining: Vec<Running>,
    /// The deferred TCP stop scheduled by `stop_accepting`.
    pending_tcp_stop: Option<JoinHandle<()>>,
}

/// The sockets a generation serves on, and what it can do with them.
pub struct Serving {
    router: Arc<Router>,
    inner: Arc<Mutex<Inner>>,
    /// Set while draining: HTTP/1.x responses carry `Connection: close`.
    closing: Arc<std::sync::atomic::AtomicBool>,
}

/// How long a draining generation keeps accepting TCP (answering HTTP/1.1 with `Connection: close`) before it stops:
/// long enough for busy keep-alive clients to leave cleanly instead of racing the close.
pub const TCP_GRACE: Duration = Duration::from_secs(1);

/// Adds `Connection: close` to HTTP/1.x responses while the generation drains.
struct CloseWhileDraining(Arc<std::sync::atomic::AtomicBool>);

#[async_trait]
impl Handler for CloseWhileDraining {
    async fn handle(
        &self,
        req: &mut Request,
        depot: &mut Depot,
        res: &mut Response,
        ctrl: &mut FlowCtrl,
    ) {
        ctrl.call_next(req, depot, res).await;
        if self.0.load(std::sync::atomic::Ordering::SeqCst)
            && matches!(
                req.version(),
                salvo::http::Version::HTTP_10 | salvo::http::Version::HTTP_11
            )
        {
            res.headers_mut().insert(
                salvo::http::header::CONNECTION,
                HeaderValue::from_static("close"),
            );
        }
    }
}

fn rustls_config(files: &TlsFiles) -> anyhow::Result<RustlsConfig> {
    let keycert = Keycert::new()
        .cert_from_path(&files.cert)
        .with_context(|| format!("reading certificate {}", files.cert.display()))?
        .key_from_path(&files.key)
        .with_context(|| format!("reading private key {}", files.key.display()))?;
    let config = RustlsConfig::new(keycert);
    // Fail here, not in the listener, when the files do not make a usable config.
    let _: salvo::conn::rustls::ServerConfig = config
        .clone()
        .try_into()
        .with_context(|| format!("loading {}", files.cert.display()))?;
    Ok(config)
}

fn spawn<A>(acceptor: A, router: &Arc<Router>) -> Running
where
    A: Acceptor + Send + 'static,
{
    let server = Server::new(acceptor);
    let handle = server.handle();
    let service = Service::new(router.clone());
    let task = tokio::spawn(async move {
        if let Err(err) = server.try_serve(service).await {
            tracing::warn!(event = "server_stopped", error = %err);
        }
    });
    Running { handle, task }
}

/// A Salvo `Listener` over a TCP listener that is already bound.
struct Prebound(std::net::TcpListener);

impl Listener for Prebound {
    type Acceptor = TcpAcceptor;

    async fn try_bind(self) -> salvo::Result<TcpAcceptor> {
        self.0.set_nonblocking(true)?;
        Ok(TcpAcceptor::try_from(tokio::net::TcpListener::from_std(
            self.0,
        )?)?)
    }
}

/// Adds `Alt-Svc: h3=":<port>"` to responses on TLS TCP listeners whose port also serves HTTP/3.
struct AltSvc {
    ports: HashMap<u16, HeaderValue>,
}

#[async_trait]
impl Handler for AltSvc {
    async fn handle(
        &self,
        req: &mut Request,
        depot: &mut Depot,
        res: &mut Response,
        ctrl: &mut FlowCtrl,
    ) {
        ctrl.call_next(req, depot, res).await;
        if *req.scheme() == Scheme::HTTPS
            && req.version() != salvo::http::Version::HTTP_3
            && let Some(port) = req.local_addr().clone().into_std().map(|addr| addr.port())
            && let Some(value) = self.ports.get(&port)
        {
            res.headers_mut().insert(ALT_SVC, value.clone());
        }
    }
}

impl TcpSlot {
    async fn start(&mut self, router: &Arc<Router>) -> anyhow::Result<()> {
        let clone = self.listener.try_clone()?;
        let running = match &self.tls {
            None => spawn(Prebound(clone).try_bind().await?, router),
            Some(files) => {
                let (tx, rx) = mpsc::unbounded_channel();
                tx.send(rustls_config(files)?).expect("receiver is alive");
                let stream = futures_util::stream::unfold(rx, |mut rx| async move {
                    rx.recv().await.map(|c| (c, rx))
                });
                let acceptor = RustlsListener::new(stream, Prebound(clone))
                    .try_bind()
                    .await?;
                self.configs = Some(tx);
                spawn(acceptor, router)
            }
        };
        self.running = Some(running);
        Ok(())
    }
}

impl UdpSlot {
    fn start(&mut self, router: &Arc<Router>) -> anyhow::Result<()> {
        if self.running.is_some() {
            return Ok(());
        }
        let config: QuinnServerConfig = rustls_config(&self.tls)?
            .try_into()
            .context("building the HTTP/3 TLS config")?;
        let socket = self.socket.try_clone()?;
        socket.set_nonblocking(true)?;
        let endpoint = quinn::Endpoint::new(
            quinn::EndpointConfig::default(),
            Some(config),
            socket,
            Arc::new(quinn::TokioRuntime),
        )?;
        let local = endpoint.local_addr()?;
        self.running = Some(spawn(
            QuinnAcceptor::new(endpoint.clone(), local, CancellationToken::new()),
            router,
        ));
        self.endpoint = Some(endpoint);
        Ok(())
    }

    /// Releases the socket: nothing in this process reads it afterwards.
    /// Releases the socket at once: closes every connection with [`UPGRADE_CLOSE_CODE`], then moves the endpoint
    /// onto a throwaway local socket, so it stops reading the shared one while its connections finish closing (a
    /// closed endpoint refuses every new connection it sees, for as long as 3 × PTO).
    async fn release(&mut self) {
        if let Some(endpoint) = self.endpoint.take() {
            // Without a server config, a handshake arriving now is dropped (the client retransmits it, and the next
            // generation answers) instead of refused.
            endpoint.set_server_config(None);
            endpoint.close(UPGRADE_CLOSE_CODE.into(), b"server upgrading, reconnect");
            // `close` only queues the close packets; they must leave from the shared socket before the rebind, or
            // clients ignore them (wrong address) and their connections hang until the idle timeout.
            tokio::time::sleep(CLOSE_FLUSH).await;
            let local = self.socket.local_addr().ok();
            let throwaway = if local.is_some_and(|a| a.is_ipv6()) {
                "[::1]:0"
            } else {
                "127.0.0.1:0"
            };
            match std::net::UdpSocket::bind(throwaway).and_then(|socket| endpoint.rebind(socket)) {
                Ok(()) => {}
                Err(err) => {
                    tracing::warn!(event = "quic_rebind_failed", error = %err, "waiting for the endpoint to go idle instead");
                    endpoint.wait_idle().await;
                }
            }
            if let Some(running) = self.running.take() {
                running.handle.stop_graceful(Some(Duration::from_secs(1)));
                let _ = running.task.await;
            }
            tokio::spawn(async move { endpoint.wait_idle().await });
        }
    }
}

impl Serving {
    /// Starts serving TCP at once, and UDP when `read_udp` (a generation started by a handoff waits for
    /// `start_udp`).
    pub async fn start(
        tcp: Vec<BoundTcp>,
        udp: Vec<BoundUdp>,
        router: Router,
        read_udp: bool,
    ) -> anyhow::Result<Serving> {
        let h3_ports: Vec<u16> = udp
            .iter()
            .filter_map(|b| b.socket.local_addr().ok())
            .map(|a| a.port())
            .collect();
        let ports = tcp
            .iter()
            .filter(|bound| bound.tls.is_some())
            .filter_map(|bound| bound.listener.local_addr().ok())
            .map(|addr: SocketAddr| addr.port())
            .filter(|port| h3_ports.contains(port))
            .map(|port| {
                (
                    port,
                    HeaderValue::from_str(&format!("h3=\":{port}\"; ma=86400")).expect("ASCII"),
                )
            })
            .collect();
        let closing = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let router = Arc::new(
            Router::new()
                .hoop(AltSvc { ports })
                .hoop(CloseWhileDraining(closing.clone()))
                .push(router),
        );

        let mut inner = Inner {
            tcp: tcp
                .into_iter()
                .map(|bound| TcpSlot {
                    listener: bound.listener,
                    tls: bound.tls,
                    configs: None,
                    running: None,
                })
                .collect(),
            udp: udp
                .into_iter()
                .map(|bound| UdpSlot {
                    socket: bound.socket,
                    tls: bound.tls,
                    endpoint: None,
                    running: None,
                })
                .collect(),
            draining: Vec::new(),
            pending_tcp_stop: None,
        };
        for slot in &mut inner.tcp {
            slot.start(&router).await?;
        }
        if read_udp {
            for slot in &mut inner.udp {
                slot.start(&router)?;
            }
        }
        Ok(Serving {
            router,
            inner: Arc::new(Mutex::new(inner)),
            closing,
        })
    }

    /// Starts reading the UDP sockets (HTTP/3). Idempotent.
    pub async fn start_udp(&self) -> anyhow::Result<()> {
        let mut inner = self.inner.lock().await;
        for slot in &mut inner.udp {
            slot.start(&self.router)?;
        }
        Ok(())
    }

    /// Starts draining: releases UDP at once (every QUIC connection gets [`UPGRADE_CLOSE_CODE`]), answers HTTP/1.1
    /// with `Connection: close`, and after [`TCP_GRACE`] stops accepting TCP (HTTP/2 gets `GOAWAY`). Open TCP
    /// connections keep being served.
    pub async fn stop_accepting(&self) {
        self.closing
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let mut inner = self.inner.lock().await;
        for slot in &mut inner.udp {
            slot.release().await;
        }
        if inner.pending_tcp_stop.is_none() {
            let shared = self.inner.clone();
            inner.pending_tcp_stop = Some(tokio::spawn(async move {
                tokio::time::sleep(TCP_GRACE).await;
                let mut inner = shared.lock().await;
                let mut stopped = Vec::new();
                for slot in &mut inner.tcp {
                    if let Some(running) = slot.running.take() {
                        running.handle.stop_graceful(None);
                        stopped.push(running);
                    }
                    slot.configs = None;
                }
                inner.draining.extend(stopped);
            }));
        }
    }

    /// Serves again after `stop_accepting`: cancels a TCP stop that has not happened yet, and starts fresh servers on
    /// fresh clones of the sockets that were stopped.
    pub async fn resume(&self) -> anyhow::Result<()> {
        let mut inner = self.inner.lock().await;
        if let Some(pending) = inner.pending_tcp_stop.take() {
            pending.abort();
        }
        self.closing
            .store(false, std::sync::atomic::Ordering::SeqCst);
        for slot in &mut inner.tcp {
            if slot.running.is_none() {
                slot.start(&self.router).await?;
            }
        }
        for slot in &mut inner.udp {
            slot.start(&self.router)?;
        }
        Ok(())
    }

    /// Re-reads every listener's certificate and key. A file that does not load is an error, and the old
    /// certificate stays.
    pub async fn reload_certs(&self) -> anyhow::Result<()> {
        let inner = self.inner.lock().await;
        let mut tcp_configs = Vec::new();
        for slot in &inner.tcp {
            if let (Some(files), Some(sender)) = (&slot.tls, &slot.configs) {
                tcp_configs.push((sender, rustls_config(files)?));
            }
        }
        let mut quic_configs = Vec::new();
        for slot in &inner.udp {
            if let Some(endpoint) = &slot.endpoint {
                let config: QuinnServerConfig = rustls_config(&slot.tls)?.try_into()?;
                quic_configs.push((endpoint, config));
            }
        }
        for (sender, config) in tcp_configs {
            let _ = sender.send(config);
        }
        for (endpoint, config) in quic_configs {
            endpoint.set_server_config(Some(config));
        }
        tracing::info!(event = "certificates_reloaded");
        Ok(())
    }

    /// Waits for the servers stopped by `stop_accepting` to finish their connections, up to `timeout`, then closes
    /// what remains.
    pub async fn wait_idle(&self, timeout: Duration) {
        let pending = self.inner.lock().await.pending_tcp_stop.take();
        if let Some(pending) = pending {
            let _ = pending.await;
        }
        let draining: Vec<Running> = std::mem::take(&mut self.inner.lock().await.draining);
        let handles: Vec<ServerHandle> = draining
            .iter()
            .map(|running| running.handle.clone())
            .collect();
        let tasks =
            futures_util::future::join_all(draining.into_iter().map(|running| running.task));
        tokio::pin!(tasks);
        if tokio::time::timeout(timeout, &mut tasks).await.is_err() {
            for handle in &handles {
                handle.stop_forceful();
            }
            let _ = tasks.await;
        }
    }
}
