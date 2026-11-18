//! A minimal HTTP/3 client for tests: one GET, whole body.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use bytes::Buf;

/// Sends `GET https://localhost:<port><path>` over HTTP/3 to `addr`, trusting `ca_pem`.
pub async fn get(addr: SocketAddr, ca_pem: &Path, path: &str) -> (u16, http::HeaderMap, Vec<u8>)
{
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = rustls::RootCertStore::empty();
    let pem = std::fs::read(ca_pem).unwrap();
    for cert in rustls_pemfile(&pem)
    {
        roots.add(cert).unwrap();
    }
    let mut tls = rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
    tls.alpn_protocols = vec![b"h3".to_vec()];
    let quic = quinn::crypto::rustls::QuicClientConfig::try_from(tls).unwrap();
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(quic)));

    let conn = endpoint.connect(addr, "localhost").unwrap().await.unwrap();
    let (mut driver, mut send) = h3::client::new(h3_quinn::Connection::new(conn)).await.unwrap();
    let drive = tokio::spawn(async move { std::future::poll_fn(|cx| driver.poll_close(cx)).await });

    let uri: http::Uri = format!("https://localhost:{}{}", addr.port(), path).parse().unwrap();
    let request = http::Request::get(uri).body(()).unwrap();
    let mut stream = send.send_request(request).await.unwrap();
    stream.finish().await.unwrap();
    let response = stream.recv_response().await.unwrap();
    let mut body = Vec::new();
    while let Some(mut chunk) = stream.recv_data().await.unwrap()
    {
        let bytes = chunk.copy_to_bytes(chunk.remaining());
        body.extend_from_slice(&bytes);
    }
    drop(send);
    drive.abort();
    endpoint.close(0u32.into(), b"done");

    (response.status().as_u16(), response.headers().clone(), body)
}

fn rustls_pemfile(pem: &[u8]) -> Vec<rustls::pki_types::CertificateDer<'static>>
{
    use rustls::pki_types::pem::PemObject;

    rustls::pki_types::CertificateDer::pem_slice_iter(pem).map(Result::unwrap).collect()
}
