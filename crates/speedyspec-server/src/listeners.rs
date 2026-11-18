//! Serving from sockets that were bound before the server started.
//!
//! The keeper binds every listening socket and hands it to a generation, so the server never binds its own. The
//! acceptors are built by [`crate::serving::Serving`]; see `docs/salvo-listeners.md` and `docs/handoff.md`.

use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use salvo::Router;

use crate::serving::Serving;

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

/// Serves `router` on every given socket until `shutdown` resolves, then stops accepting and gives in-flight
/// requests up to [`GRACEFUL_TIMEOUT`] to finish.
pub async fn serve(
    tcp: Vec<BoundTcp>,
    udp: Vec<BoundUdp>,
    router: Router,
    shutdown: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    let serving = Serving::start(tcp, udp, router, true).await?;
    shutdown.await;
    serving.stop_accepting().await;
    serving.wait_idle(GRACEFUL_TIMEOUT).await;
    Ok(())
}
