//! systemd readiness notification (`Type=notify`).

/// Sends `READY=1` to `$NOTIFY_SOCKET`, if set. A name starting with `@` is an abstract socket (Linux only).
pub fn ready() {
    let Some(path) = std::env::var_os("NOTIFY_SOCKET") else {
        return;
    };
    let socket = match std::os::unix::net::UnixDatagram::unbound() {
        Ok(socket) => socket,
        Err(err) => return tracing::warn!(event = "notify_failed", error = %err),
    };
    let text = path.to_string_lossy().into_owned();
    let result = if let Some(name) = text.strip_prefix('@') {
        abstract_send(&socket, name)
    } else {
        socket.send_to(b"READY=1", &text).map(|_| ())
    };
    if let Err(err) = result {
        tracing::warn!(event = "notify_failed", socket = %text, error = %err);
    }
}

#[cfg(target_os = "linux")]
fn abstract_send(socket: &std::os::unix::net::UnixDatagram, name: &str) -> std::io::Result<()> {
    use std::os::linux::net::SocketAddrExt;
    let addr = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes())?;
    socket.send_to_addr(b"READY=1", &addr).map(|_| ())
}

#[cfg(not(target_os = "linux"))]
fn abstract_send(_socket: &std::os::unix::net::UnixDatagram, _name: &str) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "abstract notify sockets exist only on Linux",
    ))
}
