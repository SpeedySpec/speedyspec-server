//! Proof that Salvo serves HTTP/1.1, HTTP/2 and HTTP/3 from sockets bound outside Salvo, as the keeper will hand them
//! over.

mod support;

use std::net::{SocketAddr, TcpListener, UdpSocket};
use std::time::Duration;

use salvo::prelude::*;
use speedyspec_server::listeners::{BoundTcp, BoundUdp, serve};

#[handler]
async fn ok(res: &mut Response) {
    res.render(Text::Plain("ok"));
}

fn client(ca: &std::path::Path) -> reqwest::ClientBuilder {
    let ca = reqwest::Certificate::from_pem(&std::fs::read(ca).unwrap()).unwrap();
    reqwest::Client::builder()
        .add_root_certificate(ca)
        .timeout(Duration::from_secs(10))
}

#[tokio::test]
async fn serves_all_protocols_from_prebound_sockets() {
    let dir = tempfile::tempdir().unwrap();
    let certs = support::certs::generate(dir.path());

    let plain = TcpListener::bind("127.0.0.1:0").unwrap();
    let plain_addr: SocketAddr = plain.local_addr().unwrap();
    let secure = TcpListener::bind("127.0.0.1:0").unwrap();
    let secure_addr: SocketAddr = secure.local_addr().unwrap();
    let quic = UdpSocket::bind(secure_addr).unwrap();

    let router = Router::new()
        .goal(ok)
        .push(Router::with_path("{**rest}").goal(ok));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve(
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
        vec![BoundUdp {
            socket: quic,
            tls: certs.tls.clone(),
        }],
        router,
        async move {
            let _ = stop_rx.await;
        },
    ));

    let h1 = reqwest::Client::builder().http1_only().build().unwrap();
    let response = h1
        .get(format!("http://{plain_addr}/"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.version(), reqwest::Version::HTTP_11);
    assert_eq!(response.text().await.unwrap(), "ok");

    let h2c = reqwest::Client::builder()
        .http2_prior_knowledge()
        .build()
        .unwrap();
    let response = h2c
        .get(format!("http://{plain_addr}/"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.version(), reqwest::Version::HTTP_2);

    let https = client(&certs.ca).build().unwrap();
    let response = https
        .get(format!("https://localhost:{}/", secure_addr.port()))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.version(), reqwest::Version::HTTP_2);

    let (status, _, body) = support::h3::get(secure_addr, &certs.ca, "/").await;
    assert_eq!(status, 200);
    assert_eq!(body, b"ok");

    stop_tx.send(()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("serve returns within 5 s");
    result.unwrap().unwrap();
}
