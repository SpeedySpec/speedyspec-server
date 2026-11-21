use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;
use std::thread::JoinHandle;
use std::time::Duration;

use speedyspec_data::{DbError, EngineConfig, NoSqlStore};
use speedyspec_data_conformance::nosql_store_suite;
use speedyspec_data_query::EngineRegistry;
use std::os::unix::net::UnixListener;

use speedyspec_data_redb::{
    OwnerConfig, PROTOCOL_VERSION, RedbClient, RedbEngineFactory, RedbOwner, Request, Response,
    StopHandle, read_frame, write_frame,
};

fn config(dir: &Path) -> OwnerConfig {
    OwnerConfig {
        engine: "redb".into(),
        file: dir.join("site.redb"),
        socket: dir.join("site.owner/owner.sock"),
        sweep_every: Duration::from_secs(60),
    }
}

struct Running {
    stop: StopHandle,
    thread: JoinHandle<Result<(), DbError>>,
}

fn start(cfg: &OwnerConfig) -> Running {
    let owner = RedbOwner::open(cfg.clone()).unwrap();
    let stop = owner.stop_handle();
    let thread = std::thread::spawn(move || owner.serve());
    Running { stop, thread }
}

impl Running {
    fn stop(self) {
        self.stop.stop();
        self.thread.join().unwrap().unwrap();
    }
}

fn client(cfg: &OwnerConfig) -> RedbClient {
    RedbClient::new("redb", cfg.socket.clone(), Duration::from_secs(5)).unwrap()
}

#[test]
fn redb_client_conformance() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let owner = start(&cfg);
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(RedbEngineFactory));
    registry
        .configure(EngineConfig {
            name: "redb".into(),
            url: format!("redb://{}", cfg.file.display()),
            options: BTreeMap::new(),
        })
        .unwrap();
    registry.route("test", "redb").unwrap();
    let store = registry.nosql("test").unwrap();
    nosql_store_suite(&*store, "conf");
    drop(store);
    drop(registry);
    owner.stop();
}

#[test]
fn clients_share_one_owner() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let owner = start(&cfg);
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let cfg = cfg.clone();
            std::thread::spawn(move || {
                let c = client(&cfg);
                for _ in 0..250 {
                    c.increment("ks", b"n", 1).unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let (a, b) = (client(&cfg), client(&cfg));
    assert_eq!(
        a.get("ks", b"n").unwrap().as_deref(),
        Some(b"1000".as_slice())
    );
    a.put("ks", b"shared", b"v", None).unwrap();
    assert_eq!(
        b.get("ks", b"shared").unwrap().as_deref(),
        Some(b"v".as_slice())
    );
    drop((a, b));
    owner.stop();
}

#[test]
fn owner_restart_is_transparent_for_reads() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let owner = start(&cfg);
    let c = client(&cfg);
    c.put("ks", b"k", b"v", None).unwrap();
    owner.stop();
    let owner = start(&cfg);
    assert_eq!(c.get("ks", b"k").unwrap().as_deref(), Some(b"v".as_slice()));
    drop(c);
    owner.stop();
}

#[test]
fn idle_connection_is_remade_before_increment() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let owner = start(&cfg);
    let c = client(&cfg);
    assert_eq!(c.increment("ks", b"n", 1).unwrap(), 1);
    owner.stop();
    let owner = start(&cfg);
    // Nothing was in flight when the owner went away: the increment goes to the new owner.
    assert_eq!(c.increment("ks", b"n", 1).unwrap(), 2);
    drop(c);
    owner.stop();
}

#[test]
fn increment_lost_in_flight_is_not_retried() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("fake/owner.sock");
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&socket).unwrap();
    // A fake owner that answers Hello, then hangs up on the first request without answering it.
    let fake = std::thread::spawn(move || {
        let mut requests = 0;
        for stream in listener.incoming().take(2) {
            let Ok(mut stream) = stream else { break };
            if read_frame::<Request>(&mut stream).is_err()
                || write_frame(
                    &mut stream,
                    &Response::Ready {
                        version: PROTOCOL_VERSION,
                    },
                )
                .is_err()
            {
                continue;
            }
            if read_frame::<Request>(&mut stream).is_ok() {
                requests += 1;
            }
        }
        requests
    });
    let c = RedbClient::new("redb", socket.clone(), Duration::from_secs(5)).unwrap();
    assert!(matches!(
        c.increment("ks", b"n", 1),
        Err(DbError::Connection { .. })
    ));
    drop(c);
    // Wake the fake's second accept, so it can report.
    let _ = std::os::unix::net::UnixStream::connect(&socket).map(|mut s| {
        let _ = write_frame(
            &mut s,
            &Request::Hello {
                version: PROTOCOL_VERSION,
            },
        );
    });
    assert_eq!(
        fake.join().unwrap(),
        1,
        "the increment was sent more than once"
    );
}

#[test]
fn owner_down_is_connection_error() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let err = client(&cfg).get("ks", b"k").unwrap_err();
    let socket = cfg.socket.display().to_string();
    assert!(
        matches!(&err, DbError::Connection { message, .. } if message.contains("not reachable") && message.contains(&socket)),
        "{err:?}"
    );
}
