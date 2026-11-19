//! FastCGI records, name-value pairs, and streaming between the client and the responder.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use speedyspec_fastcgi::{
    BeginRequest, Client, EndRequest, Event, Record, RecordType, Responder, decode_pairs,
    encode_pairs,
};

#[test]
fn records_round_trip() {
    let samples = [
        Record::new(
            RecordType::BeginRequest,
            1,
            BeginRequest {
                role: 1,
                keep_conn: true,
            }
            .encode()
            .to_vec(),
        ),
        Record::new(RecordType::Params, 7, b"\x04\x03HOSTabc".to_vec()),
        Record::new(RecordType::Stdin, 7, Vec::new()),
        Record::new(RecordType::Stdout, 65535, vec![b'x'; 65535]),
        Record::new(RecordType::Stderr, 2, b"oops".to_vec()),
        Record::new(
            RecordType::EndRequest,
            7,
            EndRequest {
                app_status: 3,
                protocol_status: 0,
            }
            .encode()
            .to_vec(),
        ),
        Record::new(
            RecordType::GetValues,
            0,
            encode_pairs(&[(b"FCGI_MAX_CONNS".as_slice(), b"".as_slice())]),
        ),
    ];
    let mut wire = Vec::new();
    for record in &samples {
        let bytes = record.encode();
        assert_eq!(bytes.len() % 8, 0, "records are padded to 8 bytes");
        wire.extend(bytes);
    }
    let mut reader = wire.as_slice();
    for record in &samples {
        assert_eq!(
            Record::read_from(&mut reader).unwrap().as_ref(),
            Some(record)
        );
    }
    assert_eq!(
        Record::read_from(&mut reader).unwrap(),
        None,
        "a clean end of stream"
    );
    let begin = BeginRequest::decode(&samples[0].content).unwrap();
    assert!(begin.keep_conn && begin.role == 1);
    let end = EndRequest::decode(&samples[5].content).unwrap();
    assert_eq!((end.app_status, end.protocol_status), (3, 0));
    assert!(
        Record::read_from(&mut &wire[..5]).is_err(),
        "a truncated header is an error"
    );
}

#[test]
fn name_value_pairs_long_and_short() {
    let long_name = vec![b'n'; 128];
    let long_value = vec![b'v'; 70_000];
    let pairs: Vec<(Vec<u8>, Vec<u8>)> = vec![
        (b"A".to_vec(), Vec::new()),
        (vec![b'x'; 127], b"short".to_vec()),
        (long_name.clone(), long_value.clone()),
    ];
    let encoded = encode_pairs(&pairs);
    assert_eq!(encoded[0], 1, "a short length is one byte");
    let second = 1 + 1 + 1;
    assert_eq!(encoded[second], 127);
    let third = second + 1 + 1 + 127 + 5;
    assert_eq!(
        encoded[third] & 0x80,
        0x80,
        "a long length is four bytes, high bit set"
    );
    assert_eq!(decode_pairs(&encoded).unwrap(), pairs);
    assert!(
        decode_pairs(&encoded[..encoded.len() - 1]).is_err(),
        "a truncated pair is an error"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdin_streams_with_backpressure() {
    const CHUNKS: usize = 200;
    const CHUNK: usize = 64 * 1024;
    let (client_side, worker_side) = std::os::unix::net::UnixStream::pair().unwrap();
    let (go, wait) = std::sync::mpsc::channel::<()>();

    let worker = std::thread::spawn(move || {
        let mut responder = Responder::new(worker_side);
        let request = responder.next_request().unwrap().expect("a request");
        let params: Vec<String> = request
            .params
            .iter()
            .map(|(k, v)| {
                format!(
                    "{}={}",
                    String::from_utf8_lossy(k),
                    String::from_utf8_lossy(v)
                )
            })
            .collect();
        wait.recv().unwrap();
        let mut total = 0usize;
        let mut buf = vec![0u8; 8192];
        loop {
            let n = responder.read_stdin(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            total += n;
        }
        responder
            .write_stdout(format!("Status: 200 OK\r\n\r\n{}|{total}", params.join(",")).as_bytes())
            .unwrap();
        responder.write_stderr(b"a warning").unwrap();
        responder.end_request(0).unwrap();
    });

    let pulled = Arc::new(AtomicUsize::new(0));
    let counter = pulled.clone();
    let body = futures_util::stream::iter((0..CHUNKS).map(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok::<Bytes, std::io::Error>(Bytes::from(vec![b'b'; CHUNK]))
    }));
    client_side.set_nonblocking(true).unwrap();
    let stream = tokio::net::UnixStream::from_std(client_side).unwrap();
    let params = vec![(b"REQUEST_METHOD".to_vec(), b"POST".to_vec())];
    let mut response = Client::request_on(stream, params, body).await.unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;
    let before = pulled.load(Ordering::SeqCst);
    assert!(
        before < CHUNKS,
        "the body is pulled only as the worker reads it ({before} of {CHUNKS})"
    );
    go.send(()).unwrap();

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut status = None;
    while let Some(event) = response.next().await {
        match event.unwrap() {
            Event::Stdout(bytes) => stdout.extend_from_slice(&bytes),
            Event::Stderr(bytes) => stderr.extend_from_slice(&bytes),
            Event::End(end) => status = Some(end.app_status),
        }
    }
    worker.join().unwrap();
    assert_eq!(pulled.load(Ordering::SeqCst), CHUNKS);
    assert_eq!(status, Some(0));
    assert_eq!(stderr, b"a warning");
    let text = String::from_utf8(stdout).unwrap();
    assert_eq!(
        text,
        format!(
            "Status: 200 OK\r\n\r\nREQUEST_METHOD=POST|{}",
            CHUNKS * CHUNK
        )
    );
}
