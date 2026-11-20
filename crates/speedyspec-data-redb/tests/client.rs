use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;
use std::thread::JoinHandle;
use std::time::Duration;

use speedyspec_data::{DbError, EngineConfig, NoSqlStore};
use speedyspec_data_conformance::nosql_store_suite;
use speedyspec_data_query::EngineRegistry;
use speedyspec_data_redb::{OwnerConfig, RedbClient, RedbEngineFactory, RedbOwner, StopHandle};

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
fn increment_is_not_retried() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let owner = start(&cfg);
    let c = client(&cfg);
    assert_eq!(c.increment("ks", b"n", 1).unwrap(), 1);
    owner.stop();
    let owner = start(&cfg);
    assert!(matches!(
        c.increment("ks", b"n", 1),
        Err(DbError::Connection { .. })
    ));
    assert_eq!(c.get("ks", b"n").unwrap().as_deref(), Some(b"1".as_slice()));
    assert_eq!(c.increment("ks", b"n", 1).unwrap(), 2);
    drop(c);
    owner.stop();
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
