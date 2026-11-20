use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_data::{DbError, EngineConfig, SqlStore};
use speedyspec_data_conformance::sql_store_suite;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::{EngineRegistry, parse_one};

fn store(name: &str) -> Rc<dyn SqlStore> {
    let url = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL").expect("set SPEEDYSPEC_TEST_MYSQL_URL");
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(MySqlEngineFactory));
    registry
        .configure(EngineConfig {
            name: name.into(),
            url,
            options: BTreeMap::new(),
        })
        .unwrap();
    registry.route("test", name).unwrap();
    registry.sql("test").unwrap()
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn mysql_conformance() {
    let store = store("default");
    sql_store_suite(&*store, "speedyspec_conformance");
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn reconnects_after_kill() {
    let store = store("default");
    let id = store
        .query(&parse_one("SELECT CONNECTION_ID()").unwrap())
        .unwrap();
    let id = String::from_utf8(id.rows[0].get_index(0).unwrap().to_mysql_text().unwrap()).unwrap();
    let killer = self::store("killer");
    killer
        .execute(&parse_one(&format!("KILL CONNECTION {id}")).unwrap())
        .unwrap();
    let first = store.query(&parse_one("SELECT 1").unwrap());
    assert!(
        matches!(first, Err(DbError::Connection { .. })),
        "{first:?}"
    );
    let second = store.query(&parse_one("SELECT 1").unwrap()).unwrap();
    assert_eq!(second.rows.len(), 1);
}
