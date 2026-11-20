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
