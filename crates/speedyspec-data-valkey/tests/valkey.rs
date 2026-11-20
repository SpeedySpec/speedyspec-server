use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use speedyspec_data::{DbError, EngineConfig, NoSqlStore};
use speedyspec_data_conformance::nosql_store_suite;
use speedyspec_data_query::EngineRegistry;
use speedyspec_data_valkey::{ValkeyEngineFactory, ValkeyStore, driver_url};

/// The live tests run one at a time: one of them kills every client connection.
static LIVE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn url() -> String {
    std::env::var("SPEEDYSPEC_TEST_VALKEY_URL").expect("set SPEEDYSPEC_TEST_VALKEY_URL")
}

fn prefix(name: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("speedyspec-test-{}-{nanos}-{name}:", std::process::id())
}

fn store(prefix: &str) -> Rc<dyn NoSqlStore> {
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(ValkeyEngineFactory));
    registry
        .configure(EngineConfig {
            name: "valkey".into(),
            url: url(),
            options: BTreeMap::from([("prefix".to_string(), prefix.to_string())]),
        })
        .unwrap();
    registry.route("test", "valkey").unwrap();
    registry.nosql("test").unwrap()
}

#[test]
fn url_mapping() {
    assert_eq!(
        driver_url("valkey://:pw@h:1/3", false).unwrap(),
        "redis://:pw@h:1/3"
    );
    assert_eq!(driver_url("valkey://h", true).unwrap(), "rediss://h");
    assert!(matches!(
        ValkeyStore::new(
            "v",
            "valkey://127.0.0.1:1",
            "bad prefix!",
            Duration::from_secs(1)
        ),
        Err(DbError::Config(_))
    ));
    let store = ValkeyStore::new(
        "v",
        "valkey://127.0.0.1:1",
        "p:",
        Duration::from_millis(100),
    )
    .unwrap();
    assert!(matches!(store.get("A b", b"k"), Err(DbError::Config(_))));
    assert!(matches!(store.clear("A b"), Err(DbError::Config(_))));
}

#[test]
#[ignore = "needs Valkey: set SPEEDYSPEC_TEST_VALKEY_URL"]
fn valkey_conformance() {
    let _live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    let store = store(&prefix("conf"));
    nosql_store_suite(&*store, "conf");
}

#[test]
#[ignore = "needs Valkey: set SPEEDYSPEC_TEST_VALKEY_URL"]
fn clear_keeps_other_prefixes() {
    let _live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    let base = prefix("clear");
    let (one, two) = (store(&format!("{base}p1:")), store(&format!("{base}p2:")));
    one.put("ks", b"k", b"1", None).unwrap();
    two.put("ks", b"k", b"2", None).unwrap();
    one.clear("ks").unwrap();
    assert_eq!(one.get("ks", b"k").unwrap(), None);
    assert_eq!(
        two.get("ks", b"k").unwrap().as_deref(),
        Some(b"2".as_slice())
    );
}

fn kill_clients() {
    let client = redis::Client::open(url().replacen("valkey://", "redis://", 1)).unwrap();
    let mut conn = client.get_connection().unwrap();
    let id: i64 = redis::cmd("CLIENT").arg("ID").query(&mut conn).unwrap();
    let _: i64 = redis::cmd("CLIENT")
        .arg("KILL")
        .arg("TYPE")
        .arg("normal")
        .arg("SKIPME")
        .arg("yes")
        .query(&mut conn)
        .unwrap();
    let _ = id;
}

#[test]
#[ignore = "needs Valkey: set SPEEDYSPEC_TEST_VALKEY_URL"]
fn reconnects_after_client_kill() {
    let _live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    let store = store(&prefix("kill"));
    store.put("ks", b"k", b"v", None).unwrap();
    store.put("ks", b"n", b"1", None).unwrap();
    kill_clients();
    assert_eq!(
        store.get("ks", b"k").unwrap().as_deref(),
        Some(b"v".as_slice())
    );
    kill_clients();
    assert!(matches!(
        store.increment("ks", b"n", 1),
        Err(DbError::Connection { .. })
    ));
    assert_eq!(
        store.get("ks", b"n").unwrap().as_deref(),
        Some(b"1".as_slice())
    );
    assert_eq!(store.increment("ks", b"n", 1).unwrap(), 2);
}
