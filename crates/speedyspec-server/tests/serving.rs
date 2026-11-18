//! The Serving controller: pause, release UDP, resume, wait idle, reload certificates, Alt-Svc.

mod support;

use std::net::{SocketAddr, TcpListener, UdpSocket};
use std::time::{Duration, Instant};

use salvo::prelude::*;
use speedyspec_server::listeners::{BoundTcp, BoundUdp};
use speedyspec_server::serving::{Serving, UPGRADE_CLOSE_CODE};

#[handler]
async fn ok(res: &mut Response) {
    res.render(Text::Plain("ok"));
}

#[handler]
async fn slow(res: &mut Response) {
    tokio::time::sleep(Duration::from_millis(1500)).await;
    res.render(Text::Plain("slow done"));
}

fn router() -> Router {
    Router::new()
        .get(ok)
        .push(Router::with_path("slow").get(slow))
        .push(Router::with_path("{**rest}").get(ok))
}

struct Setup {
    _dir: tempfile::TempDir,
    certs: support::certs::TestCerts,
    plain: SocketAddr,
    secure: SocketAddr,
    serving: Serving,
}

async fn setup(read_udp: bool, http3: bool) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let certs = support::certs::generate(dir.path());
    let plain = TcpListener::bind("127.0.0.1:0").unwrap();
    let secure = TcpListener::bind("127.0.0.1:0").unwrap();
    let (plain_addr, secure_addr) = (plain.local_addr().unwrap(), secure.local_addr().unwrap());
    let udp = if http3 {
        vec![BoundUdp {
            socket: UdpSocket::bind(secure_addr).unwrap(),
            tls: certs.tls.clone(),
        }]
    } else {
        vec![]
    };
    let serving = Serving::start(
        vec![
            BoundTcp {
                listener: plain,
                tls: None,
            },
            BoundTcp {
                listener: secure,
                tls: Some(certs.tls.clone()),
            },
        ],
        udp,
        router(),
        read_udp,
    )
    .await
    .unwrap();
    Setup {
        _dir: dir,
        certs,
        plain: plain_addr,
        secure: secure_addr,
        serving,
    }
}

fn https(ca: &std::path::Path) -> reqwest::Client {
    let ca = reqwest::Certificate::from_pem(&std::fs::read(ca).unwrap()).unwrap();
    reqwest::Client::builder()
        .add_root_certificate(ca)
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}

async fn h1(addr: SocketAddr, path: &str) -> reqwest::Result<String> {
    let client = reqwest::Client::builder()
        .http1_only()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    client
        .get(format!("http://{addr}{path}"))
        .send()
        .await?
        .text()
        .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_then_resume_serves_again() {
    let s = setup(true, true).await;
    let h2c = reqwest::Client::builder()
        .http2_prior_knowledge()
        .build()
        .unwrap();
    assert_eq!(h1(s.plain, "/").await.unwrap(), "ok");
    assert_eq!(
        h2c.get(format!("http://{}/", s.plain))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "ok"
    );
    assert_eq!(
        https(&s.certs.ca)
            .get(format!("https://localhost:{}/", s.secure.port()))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "ok"
    );
    assert_eq!(support::h3::get(s.secure, &s.certs.ca, "/").await.2, b"ok");

    s.serving.stop_accepting().await;
    let plain = s.plain;
    let during = tokio::spawn(async move { h1(plain, "/").await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!during.is_finished(), "nothing is accepted while stopped");

    s.serving.resume().await.unwrap();
    assert_eq!(
        during.await.unwrap().unwrap(),
        "ok",
        "a connection made while stopped is served after resume"
    );
    assert_eq!(
        https(&s.certs.ca)
            .get(format!("https://localhost:{}/", s.secure.port()))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "ok"
    );
    assert_eq!(support::h3::get(s.secure, &s.certs.ca, "/").await.2, b"ok");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn udp_waits_for_start_udp() {
    let s = setup(false, true).await;
    let early = support::h3::try_get(s.secure, &s.certs.ca, "/");
    assert!(
        tokio::time::timeout(Duration::from_millis(700), early)
            .await
            .is_err(),
        "no HTTP/3 before start_udp"
    );
    s.serving.start_udp().await.unwrap();
    s.serving.start_udp().await.unwrap();
    assert_eq!(support::h3::get(s.secure, &s.certs.ca, "/").await.2, b"ok");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inflight_request_finishes_after_stop() {
    let s = setup(true, false).await;
    let plain = s.plain;
    let pending = tokio::spawn(async move { h1(plain, "/slow").await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    s.serving.stop_accepting().await;
    assert_eq!(pending.await.unwrap().unwrap(), "slow done");
    s.serving.wait_idle(Duration::from_secs(5)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn open_h3_connection_gets_upgrade_close_code() {
    let s = setup(true, true).await;
    let (_endpoint, connection) = support::h3::open(s.secure, &s.certs.ca).await;
    // Until the handshake is confirmed, QUIC may only send a bare transport close (RFC 9000 §10.2.3), so a
    // just-opened connection cannot carry the code yet.
    tokio::time::sleep(Duration::from_millis(300)).await;
    s.serving.stop_accepting().await;
    match tokio::time::timeout(Duration::from_secs(5), connection.closed())
        .await
        .unwrap()
    {
        quinn::ConnectionError::ApplicationClosed(close) => {
            assert_eq!(close.error_code, UPGRADE_CLOSE_CODE.into())
        }
        other => panic!("expected the upgrading close, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wait_idle_forces_close_after_timeout() {
    let s = setup(true, false).await;
    let plain = s.plain;
    let pending = tokio::spawn(async move { h1(plain, "/slow").await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    s.serving.stop_accepting().await;
    let started = Instant::now();
    s.serving.wait_idle(Duration::from_millis(300)).await;
    assert!(
        started.elapsed() < Duration::from_millis(1200),
        "{:?}",
        started.elapsed()
    );
    assert!(
        pending.await.unwrap().is_err(),
        "the slow request was cut off"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reload_certs_serves_new_certificate() {
    let s = setup(true, true).await;
    let first_ca = s.certs.ca.clone();
    let first = std::fs::read(&first_ca).unwrap();
    let url = format!("https://localhost:{}/", s.secure.port());
    assert!(https(&first_ca).get(&url).send().await.is_ok());

    let next = support::certs::generate(s.certs.ca.parent().unwrap());
    assert_ne!(std::fs::read(&next.ca).unwrap(), first);
    assert!(
        https(&next.ca).get(&url).send().await.is_err(),
        "still the old certificate before the reload"
    );
    s.serving.reload_certs().await.unwrap();
    assert_eq!(
        https(&next.ca)
            .get(&url)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "ok"
    );
    assert_eq!(support::h3::get(s.secure, &next.ca, "/").await.2, b"ok");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bad_cert_keeps_old() {
    let s = setup(true, true).await;
    let url = format!("https://localhost:{}/", s.secure.port());
    std::fs::write(&s.certs.tls.cert, "not a certificate").unwrap();
    assert!(s.serving.reload_certs().await.is_err());
    assert_eq!(
        https(&s.certs.ca)
            .get(&url)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "ok"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn alt_svc_on_tls_with_http3_only() {
    let with = setup(true, true).await;
    let response = https(&with.certs.ca)
        .get(format!("https://localhost:{}/", with.secure.port()))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.headers()["alt-svc"],
        format!("h3=\":{}\"; ma=86400", with.secure.port())
    );
    let plain = reqwest::get(format!("http://{}/", with.plain))
        .await
        .unwrap();
    assert!(plain.headers().get("alt-svc").is_none());

    let without = setup(true, false).await;
    let response = https(&without.certs.ca)
        .get(format!("https://localhost:{}/", without.secure.port()))
        .send()
        .await
        .unwrap();
    assert!(response.headers().get("alt-svc").is_none());
}
