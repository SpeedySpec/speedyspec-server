//! Serving from sockets that were bound before the server started.
//!
//! The keeper binds every listening socket and hands it to a generation, so the server never binds its own. Salvo's
//! acceptors are built here from those sockets: plain TCP (HTTP/1.1 and h2c), TCP with rustls (HTTP/1.1 and HTTP/2
//! through ALPN), and a QUIC endpoint over the given UDP socket (HTTP/3). See `docs/salvo-listeners.md`.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use salvo::conn::quinn::{QuinnAcceptor, ServerConfig as QuinnServerConfig};
use salvo::conn::rustls::{Keycert, RustlsConfig, RustlsListener};
use salvo::conn::tcp::TcpAcceptor;
use salvo::conn::{Acceptor, Listener};
use salvo::server::ServerHandle;
use salvo::{Router, Server, Service};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

/// How long in-flight requests get to finish once shutdown starts.
pub const GRACEFUL_TIMEOUT: Duration = Duration::from_secs(30);

/// A certificate chain and private key, both PEM files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlsFiles {
    pub cert: PathBuf,
    pub key: PathBuf,
}

/// A bound TCP listener; with `tls` it serves HTTPS, without it cleartext HTTP/1.1 and h2c.
#[derive(Debug)]
pub struct BoundTcp {
    pub listener: std::net::TcpListener,
    pub tls: Option<TlsFiles>,
}

/// A bound UDP socket that serves HTTP/3. HTTP/3 always needs TLS.
#[derive(Debug)]
pub struct BoundUdp {
    pub socket: std::net::UdpSocket,
    pub tls: TlsFiles,
}

/// A Salvo `Listener` over a TCP listener that is already bound.
struct Prebound(std::net::TcpListener);

impl Listener for Prebound {
    type Acceptor = TcpAcceptor;

    async fn try_bind(self) -> salvo::Result<TcpAcceptor> {
        self.0.set_nonblocking(true)?;
        let listener = tokio::net::TcpListener::from_std(self.0)?;

        Ok(TcpAcceptor::try_from(listener)?)
    }
}

fn rustls_config(files: &TlsFiles) -> anyhow::Result<RustlsConfig> {
    let keycert = Keycert::new()
        .cert_from_path(&files.cert)
        .with_context(|| format!("reading certificate {}", files.cert.display()))?
        .key_from_path(&files.key)
        .with_context(|| format!("reading private key {}", files.key.display()))?;

    Ok(RustlsConfig::new(keycert))
}

fn quinn_acceptor(bound: BoundUdp) -> anyhow::Result<QuinnAcceptor> {
    let config: QuinnServerConfig = rustls_config(&bound.tls)?
        .try_into()
        .context("building the HTTP/3 TLS config")?;
    bound.socket.set_nonblocking(true)?;
    let endpoint = quinn::Endpoint::new(
        quinn::EndpointConfig::default(),
        Some(config),
        bound.socket,
        Arc::new(quinn::TokioRuntime),
    )?;
    let local = endpoint.local_addr()?;

    Ok(QuinnAcceptor::new(
        endpoint,
        local,
        CancellationToken::new(),
    ))
}

fn spawn<A>(
    tasks: &mut JoinSet<std::io::Result<()>>,
    handles: &mut Vec<ServerHandle>,
    acceptor: A,
    service: Service,
) where
    A: Acceptor + Send + 'static,
{
    let server = Server::new(acceptor);
    handles.push(server.handle());
    tasks.spawn(async move { server.try_serve(service).await });
}

/// Serves `router` on every given socket until `shutdown` resolves, then stops accepting and gives in-flight
/// requests up to [`GRACEFUL_TIMEOUT`] to finish.
pub async fn serve(
    tcp: Vec<BoundTcp>,
    udp: Vec<BoundUdp>,
    router: Arc<Router>,
    shutdown: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    let mut tasks = JoinSet::new();
    let mut handles = Vec::new();

    for bound in tcp {
        match bound.tls {
            None => spawn(
                &mut tasks,
                &mut handles,
                Prebound(bound.listener).try_bind().await?,
                Service::new(router.clone()),
            ),
            Some(files) => {
                let acceptor =
                    RustlsListener::new(rustls_config(&files)?, Prebound(bound.listener))
                        .try_bind()
                        .await?;
                spawn(
                    &mut tasks,
                    &mut handles,
                    acceptor,
                    Service::new(router.clone()),
                );
            }
        }
    }
    for bound in udp {
        spawn(
            &mut tasks,
            &mut handles,
            quinn_acceptor(bound)?,
            Service::new(router.clone()),
        );
    }

    tokio::select! {
        () = shutdown => {}
        Some(result) = tasks.join_next() => {
            result.context("server task panicked")?.context("server stopped")?;
        }
    }
    for handle in &handles {
        handle.stop_graceful(GRACEFUL_TIMEOUT);
    }
    while let Some(result) = tasks.join_next().await {
        result
            .context("server task panicked")?
            .context("server stopped with an error")?;
    }

    Ok(())
}
