//! The SQL Store conformance suite on SQLite, in memory and on a file.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use speedyspec_data::{EngineConfig, SqlStore};
use speedyspec_data_conformance::sql_store_suite;
use speedyspec_data_query::EngineRegistry;
use speedyspec_data_sqlite::{SqliteEngineFactory, SqliteStore};

fn store(url: &str) -> Rc<dyn SqlStore> {
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(SqliteEngineFactory));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url: url.into(),
            options: BTreeMap::new(),
        })
        .unwrap();
    registry.route("test", "default").unwrap();
    registry.sql("test").unwrap()
}

#[test]
fn sqlite_conformance_in_memory() {
    let store = store("sqlite://:memory:");
    assert_eq!(store.engine(), "default");
    sql_store_suite(&*store, "speedyspec_conformance");
}

#[test]
fn sqlite_conformance_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("site.db");
    let url = format!("sqlite://{}", path.display());
    let store = store(&url);
    sql_store_suite(&*store, "speedyspec_conformance");
    let other = rusqlite::Connection::open(&path).unwrap();
    let mode: String = other
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    let direct = SqliteStore::open(&EngineConfig {
        name: "direct".into(),
        url,
        options: BTreeMap::new(),
    })
    .unwrap();
    assert_eq!(direct.busy_timeout(), Duration::from_secs(5));
}
