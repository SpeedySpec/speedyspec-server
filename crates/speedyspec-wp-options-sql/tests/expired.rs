use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Mutex;

use speedyspec_data::EngineConfig;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::EngineRegistry;
use speedyspec_wp_db::{MemoryWpdbHost, MemoryWpdbState, WpDb, WpDbConfig, WpDbParts};
use speedyspec_wp_hooks::{HookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_options::{ExpiredTransients, TransientPrefix};
use speedyspec_wp_options_sql::SqlExpiredTransients;

/// The tests share `wp_options` and `wp_sitemeta`: one at a time.
static TABLES: Mutex<()> = Mutex::new(());

const OPTIONS: &str = "CREATE TABLE wp_options (option_id bigint(20) unsigned NOT NULL auto_increment, \
    option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, \
    autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), UNIQUE KEY option_name (option_name), \
    KEY autoload (autoload)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci";

const SITEMETA: &str = "CREATE TABLE wp_sitemeta (meta_id bigint(20) unsigned NOT NULL auto_increment, \
    site_id bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, meta_value longtext, \
    PRIMARY KEY (meta_id), KEY meta_key (meta_key(191)), KEY site_id (site_id)) \
    DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci";

fn rig() -> Rc<WpDb> {
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
    let hooks = Rc::new(HookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let db = Rc::new(WpDb::new(WpDbParts {
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
    for (table, schema) in [("wp_options", OPTIONS), ("wp_sitemeta", SITEMETA)] {
        db.query(&format!("DROP TABLE IF EXISTS {table}")).unwrap();
        db.query(schema).unwrap();
    }
    db
}

/// The rows: `a` expired, `b` live, `c` with no timeout, the site transient `d` expired.
const ROWS: [(&str, &str); 7] = [
    ("_transient_a", "A"),
    ("_transient_timeout_a", "999"),
    ("_transient_b", "B"),
    ("_transient_timeout_b", "2000"),
    ("_transient_c", "C"),
    ("_site_transient_d", "D"),
    ("_site_transient_timeout_d", "999"),
];

fn names(db: &WpDb, sql: &str) -> Vec<String> {
    db.query(sql).unwrap();
    db.get_col(0)
        .into_iter()
        .flatten()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .collect()
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn purges_only_expired_pairs() {
    let _one = TABLES.lock().unwrap_or_else(|e| e.into_inner());
    let db = rig();
    for (name, value) in ROWS {
        db.query(&format!("INSERT INTO wp_options (option_name, option_value, autoload) VALUES ('{name}', '{value}', 'off')"))
            .unwrap();
    }
    let purge = SqlExpiredTransients::new(db.clone());
    assert_eq!(
        purge
            .purge_options(TransientPrefix::Transient, 1000)
            .unwrap(),
        2
    );
    let left = names(
        &db,
        "SELECT option_name FROM wp_options ORDER BY option_name",
    );
    assert_eq!(
        left,
        [
            "_site_transient_d",
            "_site_transient_timeout_d",
            "_transient_b",
            "_transient_c",
            "_transient_timeout_b"
        ]
    );
    assert_eq!(
        purge
            .purge_options(TransientPrefix::SiteTransient, 1000)
            .unwrap(),
        2
    );
    let left = names(
        &db,
        "SELECT option_name FROM wp_options ORDER BY option_name",
    );
    assert_eq!(
        left,
        ["_transient_b", "_transient_c", "_transient_timeout_b"]
    );
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn purges_sitemeta() {
    let _one = TABLES.lock().unwrap_or_else(|e| e.into_inner());
    let db = rig();
    for (key, value) in [
        ("_site_transient_a", "A"),
        ("_site_transient_timeout_a", "999"),
        ("_site_transient_b", "B"),
        ("_site_transient_timeout_b", "2000"),
        ("_site_transient_c", "C"),
    ] {
        db.query(&format!(
            "INSERT INTO wp_sitemeta (site_id, meta_key, meta_value) VALUES (1, '{key}', '{value}')"
        ))
        .unwrap();
    }
    let purge = SqlExpiredTransients::new(db.clone());
    assert_eq!(purge.purge_sitemeta(1000).unwrap(), 2);
    let left = names(&db, "SELECT meta_key FROM wp_sitemeta ORDER BY meta_key");
    assert_eq!(
        left,
        [
            "_site_transient_b",
            "_site_transient_c",
            "_site_transient_timeout_b"
        ]
    );
}
