//! Proof of what the socket handoff needs (plan 3, Task 1): pausing and resuming accepts on one listener, sharing
//! listening sockets across processes, and handing a QUIC endpoint over (with a close code) or rebuilding it.

mod support;

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, UdpSocket};
use std::os::fd::{AsFd, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use salvo::conn::Acceptor;
use salvo::conn::quinn::{QuinnAcceptor, ServerConfig as QuinnServerConfig};
use salvo::conn::rustls::{Keycert, RustlsConfig};
use salvo::conn::tcp::TcpAcceptor;
use salvo::prelude::*;
use salvo::server::ServerHandle;
use tokio_util::sync::CancellationToken;

/// Application close code a draining generation sends on every QUIC connection.
const UPGRADING: u32 = 0x5353;

#[handler]
async fn named(depot: &mut Depot, res: &mut Response) {
    res.render(Text::Plain(
        depot.get::<&str>("name").copied().unwrap_or("?"),
    ));
}

#[handler]
async fn slow(res: &mut Response) {
    tokio::time::sleep(Duration::from_millis(1000)).await;
    res.render(Text::Plain("slow done"));
}

struct Name(&'static str);

#[async_trait]
impl Handler for Name {
    async fn handle(
        &self,
        req: &mut Request,
        depot: &mut Depot,
        res: &mut Response,
        ctrl: &mut FlowCtrl,
    ) {
        depot.insert("name", self.0);
        ctrl.call_next(req, depot, res).await;
    }
}

fn router(name: &'static str) -> Router {
    Router::new()
        .hoop(Name(name))
        .get(named)
        .push(Router::with_path("slow").get(slow))
}

/// Serves `name` on `acceptor`; returns the handle and the task.
fn spawn_server<A: Acceptor + Send + 'static>(
    acceptor: A,
    name: &'static str,
) -> (ServerHandle, tokio::task::JoinHandle<()>) {
    let server = Server::new(acceptor);
    let handle = server.handle();
    let task = tokio::spawn(async move {
        let _ = server.try_serve(router(name)).await;
    });
    (handle, task)
}

fn tcp_acceptor(listener: &TcpListener) -> TcpAcceptor {
    let clone = listener.try_clone().unwrap();
    clone.set_nonblocking(true).unwrap();
    TcpAcceptor::try_from(tokio::net::TcpListener::from_std(clone).unwrap()).unwrap()
}

fn quinn_config(certs: &support::certs::TestCerts) -> QuinnServerConfig {
    let keycert = Keycert::new()
        .cert_from_path(&certs.tls.cert)
        .unwrap()
        .key_from_path(&certs.tls.key)
        .unwrap();
    RustlsConfig::new(keycert).try_into().unwrap()
}

fn quinn_endpoint(socket: &UdpSocket, certs: &support::certs::TestCerts) -> quinn::Endpoint {
    let clone = socket.try_clone().unwrap();
    clone.set_nonblocking(true).unwrap();
    quinn::Endpoint::new(
        quinn::EndpointConfig::default(),
        Some(quinn_config(certs)),
        clone,
        Arc::new(quinn::TokioRuntime),
    )
    .unwrap()
}

async fn get(addr: SocketAddr, path: &str) -> String {
    let client = reqwest::Client::builder()
        .http1_only()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    client
        .get(format!("http://{addr}{path}"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
}

#[tokio::test]
async fn tcp_stops_accepting_then_resumes() {
    if std::env::var("HANDOFF_CHILD").is_ok() {
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (first, first_task) = spawn_server(tcp_acceptor(&listener), "first");
    assert_eq!(get(addr, "/").await, "first");

    let inflight = tokio::spawn(async move { get(addr, "/slow").await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    first.stop_graceful(None);
    let queued = tokio::spawn(async move { get(addr, "/").await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!queued.is_finished(), "nothing accepts while paused");

    let (second, second_task) = spawn_server(tcp_acceptor(&listener), "second");
    assert_eq!(
        queued.await.unwrap(),
        "second",
        "a connection made during the pause is served after resume"
    );
    assert_eq!(
        inflight.await.unwrap(),
        "slow done",
        "the in-flight request finishes after stop"
    );
    first_task.await.unwrap();
    second.stop_graceful(None);
    second_task.await.unwrap();
}

fn spawn_child(mode: &str, extra_env: &[(&str, String)]) -> (Child, UnixStream) {
    let (parent, child) = UnixStream::pair().unwrap();
    let child_fd = child.into_raw_fd();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "child_entry", "--nocapture", "--test-threads=1"])
        .env("HANDOFF_CHILD", mode)
        .stdout(Stdio::null());
    for (key, value) in extra_env {
        command.env(key, value);
    }
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(child_fd, 3) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let process = command.spawn().unwrap();
    unsafe { drop(OwnedFd::from_raw_fd(child_fd)) };
    (process, parent)
}

fn read_line(sock: &UnixStream) -> String {
    let mut line = String::new();
    BufReader::new(sock.try_clone().unwrap())
        .read_line(&mut line)
        .unwrap();
    line.trim().to_string()
}

/// Runs only as the child half of a proof: receives the sockets on fd 3 and serves as "child".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn child_entry() {
    let Ok(mode) = std::env::var("HANDOFF_CHILD") else {
        return;
    };
    let channel = unsafe { UnixStream::from_raw_fd(3) };
    let fds = support::fdpass::recv(&channel);
    let mut out = channel.try_clone().unwrap();
    match mode.as_str() {
        "tcp" => {
            let listener = TcpListener::from(fds.into_iter().next().unwrap());
            spawn_server(tcp_acceptor(&listener), "child");
            writeln!(out, "ready").unwrap();
        }
        "h3" => {
            let socket = UdpSocket::from(fds.into_iter().nth(1).unwrap());
            writeln!(out, "ready").unwrap();
            assert_eq!(read_line(&channel), "start");
            let certs = support::certs::TestCerts {
                tls: speedyspec_server::listeners::TlsFiles {
                    cert: std::env::var("CERT").unwrap().into(),
                    key: std::env::var("KEY").unwrap().into(),
                },
                ca: Default::default(),
            };
            let endpoint = quinn_endpoint(&socket, &certs);
            let local = endpoint.local_addr().unwrap();
            spawn_server(
                QuinnAcceptor::new(endpoint, local, CancellationToken::new()),
                "child",
            );
            writeln!(out, "started").unwrap();
        }
        other => panic!("unknown child mode {other}"),
    }
    let reader = channel.try_clone().unwrap();
    tokio::task::spawn_blocking(move || {
        let mut rest = String::new();
        let _ = BufReader::new(reader).read_line(&mut rest);
    })
    .await
    .unwrap();
    std::process::exit(0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_processes_share_a_listener() {
    if std::env::var("HANDOFF_CHILD").is_ok() {
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (parent, parent_task) = spawn_server(tcp_acceptor(&listener), "parent");
    assert_eq!(get(addr, "/").await, "parent");

    let (mut child, channel) = spawn_child("tcp", &[]);
    support::fdpass::send(&channel, &[listener.as_fd(), udp.as_fd()]);
    assert_eq!(read_line(&channel), "ready");
    parent.stop_graceful(None);
    parent_task.await.unwrap();
    for _ in 0..20 {
        assert_eq!(get(addr, "/").await, "child");
    }
    drop(channel);
    assert!(child.wait().unwrap().success());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quic_endpoint_hands_over() {
    if std::env::var("HANDOFF_CHILD").is_ok() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let certs = support::certs::generate(dir.path());
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = udp.local_addr().unwrap();
    let endpoint = quinn_endpoint(&udp, &certs);
    let (parent, parent_task) = spawn_server(
        QuinnAcceptor::new(endpoint.clone(), addr, CancellationToken::new()),
        "parent",
    );
    let (_, _, body) = support::h3::get(addr, &certs.ca, "/").await;
    assert_eq!(body, b"parent");
    let (_client, open) = support::h3::open(addr, &certs.ca).await;

    let (mut child, channel) = spawn_child(
        "h3",
        &[
            ("CERT", certs.tls.cert.display().to_string()),
            ("KEY", certs.tls.key.display().to_string()),
        ],
    );
    support::fdpass::send(&channel, &[tcp.as_fd(), udp.as_fd()]);
    assert_eq!(read_line(&channel), "ready");

    endpoint.close(UPGRADING.into(), b"server upgrading, reconnect");
    // `close` only queues the close on each connection's driver task. Stopping the server first would race it: the
    // server closes its connections itself (with its own code) as it stops. So the client must have the upgrading
    // close before the server stops.
    match tokio::time::timeout(Duration::from_secs(5), open.closed())
        .await
        .unwrap()
    {
        quinn::ConnectionError::ApplicationClosed(close) => assert_eq!(
            close.error_code,
            UPGRADING.into(),
            "closed with reason {:?}",
            String::from_utf8_lossy(&close.reason)
        ),
        other => panic!("expected the upgrading close, got {other:?}"),
    }
    parent.stop_graceful(Some(Duration::from_secs(1)));
    let _ = parent_task.await;
    // The closed endpoint keeps reading the socket until its connections finish closing; only then may another
    // process's endpoint read it.
    endpoint.wait_idle().await;
    drop(endpoint);

    let mut writer = channel.try_clone().unwrap();
    writeln!(writer, "start").unwrap();
    assert_eq!(read_line(&channel), "started");
    for _ in 0..5 {
        let (_, _, body) = support::h3::try_get(addr, &certs.ca, "/")
            .await
            .expect("the first try succeeds");
        assert_eq!(body, b"child");
    }
    drop(writer);
    drop(channel);
    assert!(child.wait().unwrap().success());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quic_endpoint_resumes() {
    if std::env::var("HANDOFF_CHILD").is_ok() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let certs = support::certs::generate(dir.path());
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = udp.local_addr().unwrap();
    let first = quinn_endpoint(&udp, &certs);
    let (handle, task) = spawn_server(
        QuinnAcceptor::new(first.clone(), addr, CancellationToken::new()),
        "first",
    );
    assert_eq!(support::h3::get(addr, &certs.ca, "/").await.2, b"first");
    first.close(UPGRADING.into(), b"server upgrading, reconnect");
    handle.stop_graceful(Some(Duration::from_secs(1)));
    let _ = task.await;
    first.wait_idle().await;
    drop(first);

    let second = quinn_endpoint(&udp, &certs);
    spawn_server(
        QuinnAcceptor::new(second, addr, CancellationToken::new()),
        "second",
    );
    assert_eq!(support::h3::get(addr, &certs.ca, "/").await.2, b"second");
}
