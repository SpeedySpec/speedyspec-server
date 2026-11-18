//! The messages. The protocol changes only by adding messages: receivers skip messages they do not know, so every
//! keeper keeps speaking every version that has shipped.

use std::net::SocketAddr;
use std::path::PathBuf;

use semver::Version;
use serde::{Deserialize, Serialize};

/// The protocol version this build speaks.
pub const PROTOCOL: u32 = 1;

/// From the keeper to a generation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToGeneration {
    /// The first message, carrying one fd per socket, in order.
    Hello {
        protocol: u32,
        config: PathBuf,
        sockets: Vec<SocketInfo>,
        role: GenerationRole,
    },
    /// Start reading the UDP sockets (HTTP/3).
    StartUdp,
    /// Stop accepting, release UDP, keep serving open connections.
    Drain,
    /// The upgrade failed: serve again. `failed` is the version whose trial failed.
    Resume { failed: Option<Version> },
    /// Re-read the TLS certificates.
    ReloadCerts,
    /// Finish draining and exit.
    Shutdown,
    /// One control request line from `keeper.sock`.
    Control(String),
}

/// From a generation to the keeper.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToKeeper {
    Ready {
        version: Version,
        protocol: u32,
    },
    UdpReleased,
    /// Switch to a kept server binary.
    Upgrade {
        version: Version,
        binary: PathBuf,
    },
    /// Hand off to a new generation of the confirmed binary, re-reading the config.
    Reload,
    /// The reply to a `Control` request.
    ControlReply(String),
    Exiting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SocketKind {
    Tcp,
    Udp,
}

/// What one passed socket is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocketInfo {
    pub kind: SocketKind,
    pub address: SocketAddr,
    /// Certificate and key PEM files, for TLS TCP and for UDP (HTTP/3).
    pub tls: Option<(PathBuf, PathBuf)>,
    /// For TCP: whether HTTP/3 is served on the same port (for `Alt-Svc`).
    pub http3: bool,
    /// The readiness probe listener (`/readyz`, `/livez` only).
    #[serde(default)]
    pub probe: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GenerationRole {
    Managed,
    Immutable,
}
