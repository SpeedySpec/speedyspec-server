use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use speedyspec_data::DbError;
use speedyspec_data_redb::{
    MAX_FRAME, OwnerConfig, OwnerError, PROTOCOL_VERSION, RedbOwner, Request, Response, WireBound,
    check_socket_path, read_frame, run_owner, write_frame,
};

fn config(dir: &Path) -> OwnerConfig {
    OwnerConfig {
        engine: "redb".into(),
        file: dir.join("site.redb"),
        socket: dir.join("owner/owner.sock"),
        sweep_every: Duration::from_secs(60),
    }
}

#[test]
fn frames_roundtrip() {
    let requests = vec![
        Request::Hello {
            version: PROTOCOL_VERSION,
        },
        Request::Get {
            keyspace: "k".into(),
            key: vec![1],
        },
        Request::GetMany {
            keyspace: "k".into(),
            keys: vec![vec![1], vec![]],
        },
        Request::Put {
            keyspace: "k".into(),
            key: vec![1],
            value: vec![2],
            ttl_ms: Some(5),
        },
        Request::PutIfAbsent {
            keyspace: "k".into(),
            key: vec![1],
            value: vec![2],
            ttl_ms: None,
        },
        Request::Delete {
            keyspace: "k".into(),
            key: vec![1],
        },
        Request::Range {
            keyspace: "k".into(),
            start: WireBound::Included(vec![1]),
            end: WireBound::Unbounded,
            limit: Some(3),
        },
        Request::Increment {
            keyspace: "k".into(),
            key: vec![1],
            by: -2,
        },
        Request::Clear {
            keyspace: "k".into(),
        },
    ];
    for request in requests {
        let mut buf = Vec::new();
        write_frame(&mut buf, &request).unwrap();
        let back: Request = read_frame(&mut Cursor::new(buf)).unwrap();
        assert_eq!(back, request);
    }
    let mut big = (MAX_FRAME + 1).to_be_bytes().to_vec();
    big.extend([0; 8]);
    let err = read_frame::<Request>(&mut Cursor::new(big)).unwrap_err();
    assert!(matches!(err, DbError::Engine { code: Some(c), .. } if c == "too-large"));
}

#[test]
fn owner_socket_is_private() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let _owner = RedbOwner::open(cfg.clone()).unwrap();
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(cfg.socket.parent().unwrap()), 0o700);
    assert_eq!(mode(&cfg.socket), 0o600);
}

#[test]
fn stale_socket_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    drop(RedbOwner::open(cfg.clone()).unwrap());
    assert!(cfg.socket.exists());
    RedbOwner::open(cfg).unwrap();
}

fn serving(
    cfg: OwnerConfig,
) -> (
    speedyspec_data_redb::StopHandle,
    std::thread::JoinHandle<Result<(), DbError>>,
) {
    let owner = RedbOwner::open(cfg).unwrap();
    let stop = owner.stop_handle();
    let thread = std::thread::spawn(move || owner.serve());
    (stop, thread)
}

#[test]
fn second_owner_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let (stop, thread) = serving(cfg.clone());
    assert!(matches!(
        RedbOwner::open(cfg.clone()),
        Err(OwnerError::AlreadyRunning)
    ));
    assert_eq!(run_owner(cfg), 3);
    stop.stop();
    thread.join().unwrap().unwrap();
}

#[test]
fn rejects_bad_hello() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let (stop, thread) = serving(cfg.clone());
    let mut stream = UnixStream::connect(&cfg.socket).unwrap();
    write_frame(&mut stream, &Request::Hello { version: 99 }).unwrap();
    let reply: Response = read_frame(&mut stream).unwrap();
    assert!(matches!(reply, Response::Error { code, .. } if code == "protocol"));
    assert!(matches!(
        read_frame::<Response>(&mut stream),
        Err(DbError::Connection { .. })
    ));
    stop.stop();
    thread.join().unwrap().unwrap();
}

#[test]
fn long_socket_path_refused() {
    let long = Path::new("/tmp").join("x".repeat(115));
    let limit = if cfg!(target_os = "macos") { 103 } else { 107 };
    let err = check_socket_path(&long).unwrap_err();
    assert!(
        matches!(&err, DbError::Config(m) if m.contains(&long.display().to_string()) && m.contains(&limit.to_string())),
        "{err:?}"
    );
}

#[test]
fn stop_ends_serve() {
    let dir = tempfile::tempdir().unwrap();
    let (stop, thread) = serving(config(dir.path()));
    let other = stop.clone();
    std::thread::spawn(move || other.stop()).join().unwrap();
    thread.join().unwrap().unwrap();
}
