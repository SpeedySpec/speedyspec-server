use std::collections::BTreeMap;
use std::os::fd::AsRawFd;
use std::rc::Rc;

use speedyspec_data::{DbError, DirectStore, EngineConfig};
use speedyspec_data_conformance::direct_store_suite;
use speedyspec_data_file::{FileClient, FileEngineFactory, FileStore};
use speedyspec_data_query::EngineRegistry;

fn open(dir: &std::path::Path) -> Rc<dyn DirectStore> {
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(FileEngineFactory));
    registry
        .configure(EngineConfig {
            name: "files".into(),
            url: format!("file://{}", dir.display()),
            options: BTreeMap::new(),
        })
        .unwrap();
    registry.route("test", "files").unwrap();
    registry.direct("test").unwrap()
}

fn client(store: &dyn DirectStore) -> FileClient {
    store
        .as_any()
        .downcast_ref::<FileClient>()
        .expect("a FileClient")
        .clone()
}

fn invalid(result: Result<impl std::fmt::Debug, DbError>) -> bool {
    matches!(result, Err(DbError::Engine { code: Some(c), .. }) if c == "invalid-path")
}

#[test]
fn file_direct_conformance() {
    let dir = tempfile::tempdir().unwrap();
    direct_store_suite(&*open(&dir.path().join("root")), "file");
}

#[test]
fn as_any_exposes_client() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(&*open(dir.path()));
    c.write("a/b.txt", b"x").unwrap();
    assert_eq!(c.read("a/b.txt").unwrap().as_deref(), Some(b"x".as_slice()));
}

#[test]
fn write_is_atomic_rename() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(&*open(dir.path()));
    let big = vec![7u8; 1 << 20];
    for _ in 0..50 {
        c.write("a/b.txt", &big).unwrap();
    }
    assert_eq!(c.list("a").unwrap(), ["b.txt"]);
}

#[test]
fn refuses_escaping_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let c = client(&*open(&root));
    for bad in [
        "../x",
        "/etc/passwd",
        "a/../../x",
        "",
        "a\0b",
        "./a",
        "a//b",
    ] {
        assert!(invalid(c.write(bad, b"x")), "{bad:?}");
        assert!(invalid(c.read(bad)), "{bad:?}");
    }
    let outside: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(outside, ["root"]);
}

#[test]
fn missing_reads_are_none() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(&*open(dir.path()));
    assert_eq!(c.read("nope").unwrap(), None);
    assert!(!c.remove("nope").unwrap());
    assert!(c.list("nope").unwrap().is_empty());
    assert!(!c.rename("nope", "other").unwrap());
}

#[test]
fn closed_store_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let c = client(&*store);
    store.close().unwrap();
    assert!(matches!(c.read("x"), Err(DbError::Connection { .. })));
}

#[test]
fn lock_is_exclusive() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileStore::new("files", dir.path().to_path_buf());
    store.connect().unwrap();
    let held = store.client().lock("locks/one").unwrap();
    let other = std::fs::OpenOptions::new()
        .read(true)
        .open(dir.path().join("locks/one"))
        .unwrap();
    let try_lock = || unsafe { libc::flock(other.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    assert_ne!(try_lock(), 0);
    drop(held);
    assert_eq!(try_lock(), 0);
}
