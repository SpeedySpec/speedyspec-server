use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_data::EngineConfig;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::EngineRegistry;
use speedyspec_hooks::Key;
use speedyspec_wp_db::{
    MemoryWpdbHost, MemoryWpdbState, OutputShape, Shaped, WpDb, WpDbConfig, WpDbParts, WpValue,
};
use speedyspec_wp_hooks::{WpHookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;

fn wpdb() -> WpDb {
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
    let db = WpDb::new(WpDbParts {
        store: registry.sql("default").unwrap(),
        hooks: Rc::new(WpHookApi::new(
            Rc::new(MemoryHookStore::new()),
            Rc::new(IgnoreDeprecations),
        )),
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
    });
    db.db_connect(true).unwrap();
    db
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn round_trip_on_mysql() {
    let db = wpdb();
    db.query("DROP TABLE IF EXISTS wp_rt").unwrap();
    db.query("CREATE TABLE wp_rt (id bigint(20) unsigned NOT NULL AUTO_INCREMENT, name varchar(20) NOT NULL, n int, PRIMARY KEY (id)) DEFAULT CHARSET=utf8mb4").unwrap();
    assert_eq!(
        db.insert(
            "wp_rt",
            &[
                ("name".into(), WpValue::Str("a".into())),
                ("n".into(), WpValue::Int(5))
            ],
            None
        )
        .unwrap(),
        1
    );
    db.query("SELECT name, n FROM wp_rt").unwrap();
    match db.get_results(Some(OutputShape::ArrayA)).unwrap() {
        Shaped::List(rows) => assert_eq!(
            rows[0].fields,
            vec![
                ("name".to_string(), Some(b"a".to_vec())),
                ("n".to_string(), Some(b"5".to_vec()))
            ]
        ),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        db.update(
            "wp_rt",
            &[("name".into(), WpValue::Str("a".into()))],
            &[("id".into(), WpValue::Int(1))],
            None,
            None
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.delete("wp_rt", &[("id".into(), WpValue::Int(1))], None)
            .unwrap(),
        1
    );
    db.query("DROP TABLE wp_rt").unwrap();
    let _ = Key::Int(0);
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn server_caps_on_mysql() {
    let db = wpdb();
    assert!(db.has_cap("utf8mb4_520"));
    let version = db.db_version().unwrap();
    assert!(
        version.split('.').take(2).all(|p| p.parse::<u32>().is_ok()),
        "{version}"
    );
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn open_transaction_reset() {
    let db = wpdb();
    db.query("DROP TABLE IF EXISTS wp_tx").unwrap();
    db.query("CREATE TABLE wp_tx (a int)").unwrap();
    db.query("START TRANSACTION").unwrap();
    db.query("INSERT INTO wp_tx VALUES (1)").unwrap();
    db.store().reset().unwrap();
    db.query("SELECT COUNT(*) FROM wp_tx").unwrap();
    assert_eq!(db.get_var(0, 0), Some(b"0".to_vec()));
    db.query("DROP TABLE wp_tx").unwrap();
}
