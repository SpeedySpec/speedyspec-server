use std::time::Duration;

use speedyspec_data::NoSqlStore;
use speedyspec_data_conformance::nosql_store_suite;
use speedyspec_data_redb::RedbStore;

#[test]
fn redb_store_conformance() {
    let dir = tempfile::tempdir().unwrap();
    let store = RedbStore::open("redb", &dir.path().join("site.redb")).unwrap();
    nosql_store_suite(&store, "conf");
}

#[test]
fn sweep_removes_expired() {
    let dir = tempfile::tempdir().unwrap();
    let store = RedbStore::open("redb", &dir.path().join("site.redb")).unwrap();
    for k in [b"a", b"b", b"c"] {
        store
            .put("ks", k, b"x", Some(Duration::from_millis(10)))
            .unwrap();
    }
    store.put("ks", b"d", b"x", None).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(store.sweep_expired(100).unwrap(), 3);
    assert_eq!(
        store.get("ks", b"d").unwrap().as_deref(),
        Some(b"x".as_slice())
    );
}

#[test]
fn huge_ttl_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let store = RedbStore::open("redb", &dir.path().join("site.redb")).unwrap();
    store
        .put(
            "ks",
            b"far",
            b"x",
            Some(Duration::from_secs(i64::MAX as u64)),
        )
        .unwrap();
    assert_eq!(
        store.get("ks", b"far").unwrap().as_deref(),
        Some(b"x".as_slice())
    );
}

#[test]
fn writes_do_not_sync_each_commit() {
    let dir = tempfile::tempdir().unwrap();
    let store = RedbStore::open("redb", &dir.path().join("site.redb")).unwrap();
    let start = std::time::Instant::now();
    for i in 0..300u32 {
        store.put("ks", &i.to_be_bytes(), b"x", None).unwrap();
    }
    assert!(
        start.elapsed() < Duration::from_millis(750),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn sync_persists_unsynced_writes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("site.redb");
    {
        let store = RedbStore::open("redb", &file).unwrap();
        store.put("ks", b"k", b"v", None).unwrap();
        store.sync().unwrap();
    }
    let store = RedbStore::open("redb", &file).unwrap();
    assert_eq!(
        store.get("ks", b"k").unwrap().as_deref(),
        Some(b"v".as_slice())
    );
}

#[test]
fn sweep_all_expired_empties_every_batch() {
    let dir = tempfile::tempdir().unwrap();
    let store = RedbStore::open("redb", &dir.path().join("site.redb")).unwrap();
    for i in 0..2500u32 {
        store
            .put("ks", &i.to_be_bytes(), b"x", Some(Duration::from_millis(5)))
            .unwrap();
    }
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(store.sweep_all_expired(1000).unwrap(), 2500);
    assert_eq!(store.sweep_expired(1000).unwrap(), 0);
}
