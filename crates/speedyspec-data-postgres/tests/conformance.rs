//! The SQL Store conformance suite on PostgreSQL.

mod common;

use std::collections::BTreeMap;
use std::rc::Rc;

use common::pg_url;
use speedyspec_data::{DbError, EngineConfig, SqlStore};
use speedyspec_data_conformance::sql_store_suite;
use speedyspec_data_postgres::PostgresEngineFactory;
use speedyspec_data_query::{EngineRegistry, parse_one};

pub fn store(schema: &str) -> Rc<dyn SqlStore> {
    let mut admin = postgres::Client::connect(&pg_url(), postgres::NoTls).unwrap();
    admin
        .batch_execute(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .unwrap();
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(PostgresEngineFactory));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url: pg_url(),
            options: BTreeMap::from([("schema".to_string(), schema.to_string())]),
        })
        .unwrap();
    registry.route("test", "default").unwrap();
    registry.sql("test").unwrap()
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn postgres_conformance() {
    let store = store("t_conformance");
    sql_store_suite(&*store, "speedyspec_conformance");
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn reconnects_after_terminate() {
    let store = store("t_reconnect");
    let pid = store
        .query(&parse_one("SELECT pg_backend_pid() AS pid").unwrap())
        .unwrap();
    let pid =
        String::from_utf8(pid.rows[0].get_index(0).unwrap().to_mysql_text().unwrap()).unwrap();
    let mut killer = postgres::Client::connect(&pg_url(), postgres::NoTls).unwrap();
    killer
        .execute(&format!("SELECT pg_terminate_backend({pid})"), &[])
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    let first = store.query(&parse_one("SELECT 1").unwrap());
    assert!(
        matches!(first, Err(DbError::Connection { .. })),
        "{first:?}"
    );
    assert_eq!(
        store
            .query(&parse_one("SELECT 1").unwrap())
            .unwrap()
            .rows
            .len(),
        1
    );
}
