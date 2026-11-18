//! Against a tiny scripted HTTP/1.1 server on loopback.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc;

use speedyspec_server_domain::ReleaseSource;
use speedyspec_server_infra_http::HttpSource;

/// Serves one request with `respond(request_head) -> raw response bytes`, then closes. Returns the base URL and a
/// receiver of the request head.
fn serve_once(
    respond: impl FnOnce(&str) -> Vec<u8> + Send + 'static,
) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut head = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                break;
            }
            head.push_str(&line);
        }
        let response = respond(&head);
        let _ = stream.write_all(&response);
        let _ = stream.flush();
        let _ = tx.send(head);
    });
    (url, rx)
}

fn response(status: &str, headers: &[&str], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\nConnection: close\r\n").into_bytes();
    for header in headers {
        out.extend_from_slice(format!("{header}\r\n").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

const BODY: &[u8] = b"0123456789abcdefghij";

#[test]
fn index_fetched_and_parsed() {
    let json =
        br#"{"app":"admin","releases":[{"version":"1.0.0","published":"x","artifacts":[]}]}"#;
    let (url, _) = serve_once(move |_| {
        response(
            "200 OK",
            &[&format!("Content-Length: {}", json.len())],
            json,
        )
    });
    let index = HttpSource::new()
        .index(&format!("{url}/index.json"))
        .unwrap();
    assert_eq!(index.releases[0].version.to_string(), "1.0.0");
}

#[test]
fn rejects_plain_http_off_loopback() {
    let err = HttpSource::new()
        .index("http://example.com/index.json")
        .unwrap_err();
    assert!(
        err.to_string().contains("http://example.com/index.json"),
        "{err}"
    );
    assert!(err.to_string().contains("https"), "{err}");
}

#[test]
fn resumes_with_range() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("artifact");
    std::fs::write(&dest, &BODY[..10]).unwrap();
    let (url, head) = serve_once(|head| {
        assert!(
            head.to_ascii_lowercase().contains("range: bytes=10-"),
            "{head}"
        );
        response(
            "206 Partial Content",
            &["Content-Length: 10", "Content-Range: bytes 10-19/20"],
            &BODY[10..],
        )
    });
    HttpSource::new()
        .download(&format!("{url}/a"), &dest)
        .unwrap();
    assert!(
        head.recv()
            .unwrap()
            .to_ascii_lowercase()
            .contains("range: bytes=10-")
    );
    assert_eq!(std::fs::read(&dest).unwrap(), BODY);
}

#[test]
fn restarts_when_range_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("artifact");
    std::fs::write(&dest, &BODY[..10]).unwrap();
    let (url, _) = serve_once(|_| response("200 OK", &["Content-Length: 20"], BODY));
    HttpSource::new()
        .download(&format!("{url}/a"), &dest)
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), BODY);
}

#[test]
fn complete_file_answered_416_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("artifact");
    std::fs::write(&dest, BODY).unwrap();
    let (url, _) = serve_once(|_| {
        response(
            "416 Range Not Satisfiable",
            &["Content-Length: 0", "Content-Range: bytes */20"],
            b"",
        )
    });
    HttpSource::new()
        .download(&format!("{url}/a"), &dest)
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), BODY);
}

#[test]
fn truncated_body_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("artifact");
    let (url, _) = serve_once(|_| response("200 OK", &["Content-Length: 100"], &[b'x'; 40]));
    assert!(
        HttpSource::new()
            .download(&format!("{url}/a"), &dest)
            .is_err()
    );
    assert_eq!(std::fs::metadata(&dest).unwrap().len(), 40);
}

#[test]
fn status_error_names_url() {
    let dir = tempfile::tempdir().unwrap();
    let (url, _) = serve_once(|_| response("404 Not Found", &["Content-Length: 0"], b""));
    let target = format!("{url}/missing");
    let err = HttpSource::new()
        .download(&target, &dir.path().join("a"))
        .unwrap_err()
        .to_string();
    assert!(err.contains(&target) && err.contains("404"), "{err}");
}

