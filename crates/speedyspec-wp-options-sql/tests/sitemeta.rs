use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Mutex;

use speedyspec_data::EngineConfig;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::EngineRegistry;
use speedyspec_wp_db::{MemoryWpdbHost, MemoryWpdbState, NativeWpdb, WpDbConfig, WpDbParts};
use speedyspec_wp_hooks::{IgnoreDeprecations, WpHookApi};
use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_options::{NetworkMeta, SerializedValue, SitemetaRepository};
use speedyspec_wp_options_memory::suites::sitemeta_repository_suite;
use speedyspec_wp_options_sql::SqlSitemetaRepository;

/// The tests share one `wp_sitemeta`: one at a time.
static TABLE: Mutex<()> = Mutex::new(());

const SCHEMA: &str = "CREATE TABLE wp_sitemeta (meta_id bigint(20) unsigned NOT NULL auto_increment, \
    site_id bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, meta_value longtext, \
    PRIMARY KEY (meta_id), KEY meta_key (meta_key(191)), KEY site_id (site_id)) \
    DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci";

fn rig() -> (SqlSitemetaRepository, Rc<WpHookApi>) {
    let url = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL").expect("set SPEEDYSPEC_TEST_MYSQL_URL");
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(MySqlEngineFactory));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url,
            options: BTreeMap::new(),
        })
        .unwrap();
    let hooks = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let db = Rc::new(NativeWpdb::new(WpDbParts {
        store: registry.sql("default").unwrap(),
        hooks: hooks.clone(),
        state: Rc::new(MemoryWpdbState::new()),
        host: Rc::new(MemoryWpdbHost::new()),
        config: WpDbConfig {
            user: "root".into(),
            password: "password".into(),
            name: "speedyspec_test".into(),
            host: "127.0.0.1:33306".into(),
            charset: Some("utf8mb4".into()),
            collate: None,
            table_prefix: "wp_".into(),
        },
    }));
    db.db_connect(true).unwrap();
    db.set_prefix("wp_", true).unwrap();
    db.query("DROP TABLE IF EXISTS wp_sitemeta").unwrap();
    db.query(SCHEMA).unwrap();
    (SqlSitemetaRepository::new(db), hooks)
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn mysql_sitemeta_suite() {
    let _one = TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let (repo, _) = rig();
    sitemeta_repository_suite(&repo);
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn query_filter_fires() {
    let _one = TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let (repo, hooks) = rig();
    let seen: Rc<RefCell<Vec<String>>> = Rc::default();
    let log = seen.clone();
    hooks
        .add_filter(
            &HookName::from("query"),
            Callback::rust("count", move |args: &[HookValue]| {
                if let HookValue::Str(q) = &args[0] {
                    log.borrow_mut().push(q.clone());
                }
                Ok(args[0].clone())
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    let meta = NetworkMeta {
        network_id: 1,
        key: "k".into(),
        value: SerializedValue::new(b"v".to_vec()),
    };
    repo.add(meta.clone()).unwrap();
    repo.find(1, "k").unwrap();
    repo.find_many(1, &["k".into()]).unwrap();
    repo.exists(1, "k").unwrap();
    repo.update(NetworkMeta {
        value: SerializedValue::new(b"w".to_vec()),
        ..meta
    })
    .unwrap();
    repo.remove(1, "k").unwrap();
    // Each statement, as WordPress sends it (wpdb's own charset lookups come too).
    let seen = seen.borrow();
    for start in [
        "INSERT INTO `wp_sitemeta`",
        "SELECT meta_value FROM",
        "SELECT meta_key, meta_value FROM",
        "SELECT meta_id FROM",
        "UPDATE `wp_sitemeta`",
        "DELETE FROM `wp_sitemeta`",
    ] {
        assert!(
            seen.iter().any(|q| q.starts_with(start)),
            "{start}: {seen:?}"
        );
    }
}

/// `wp_prime_network_option_caches()`'s query, as WordPress's `prepare()` makes it.
#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn prime_query_text_is_wordpress() {
    let _one = TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let (repo, hooks) = rig();
    repo.add(NetworkMeta {
        network_id: 1,
        key: "a".into(),
        value: SerializedValue::new(b"v".to_vec()),
    })
    .unwrap();
    let seen: Rc<RefCell<Vec<String>>> = Rc::default();
    let log = seen.clone();
    hooks
        .add_filter(
            &HookName::from("query"),
            Callback::rust("see", move |args: &[HookValue]| {
                if let HookValue::Str(q) = &args[0] {
                    log.borrow_mut().push(q.clone());
                }
                Ok(args[0].clone())
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    assert_eq!(
        repo.find_many(1, &["a".into(), "b".into()]).unwrap().len(),
        1
    );
    assert_eq!(
        *seen.borrow(),
        [
            "SELECT meta_key, meta_value FROM wp_sitemeta WHERE meta_key IN ('a','b') AND site_id = 1"
        ]
    );
}
