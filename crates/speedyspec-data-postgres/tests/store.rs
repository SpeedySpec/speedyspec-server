//! The PostgreSQL store answering as MySQL does, on a live server.

mod common;

use std::collections::BTreeMap;
use std::rc::Rc;

use common::{WP_OPTIONS, WP_POSTS, pg_url};
use speedyspec_data::{DbError, DbValue, EngineConfig, ExecResult, Rows, SqlStore};
use speedyspec_data_postgres::PostgresEngineFactory;
use speedyspec_data_query::{EngineRegistry, parse_one};

fn store(name: &str) -> Rc<dyn SqlStore> {
    let schema = format!("t_{name}");
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
            options: BTreeMap::from([("schema".to_string(), schema)]),
        })
        .unwrap();
    registry.route("test", "default").unwrap();
    let store = registry.sql("test").unwrap();
    for ddl in [WP_OPTIONS, WP_POSTS] {
        exec(&*store, ddl);
    }
    store
}

fn exec(store: &dyn SqlStore, sql: &str) -> ExecResult {
    store
        .execute(&parse_one(sql).unwrap())
        .unwrap_or_else(|e| panic!("{sql}: {e:?}"))
}

fn query(store: &dyn SqlStore, sql: &str) -> Rows {
    store
        .query(&parse_one(sql).unwrap())
        .unwrap_or_else(|e| panic!("{sql}: {e:?}"))
}

fn scalar(store: &dyn SqlStore, sql: &str) -> Option<String> {
    query(store, sql)
        .rows
        .first()
        .and_then(|r| r.get_index(0))
        .and_then(DbValue::to_mysql_text)
        .map(|t| String::from_utf8(t).unwrap())
}

const UPSERT: &str = "INSERT INTO wp_options (option_name, option_value, autoload) VALUES ('a', 'b', 'auto') ON DUPLICATE KEY UPDATE option_name = VALUES(option_name), option_value = VALUES(option_value), autoload = VALUES(autoload)";

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn changed_rows_match_mysql() {
    let s = store("changed_rows");
    let first = exec(&*s, UPSERT);
    assert_eq!((first.rows_affected, first.insert_id), (1, Some(1)));
    assert_eq!(exec(&*s, UPSERT).rows_affected, 0);
    assert_eq!(exec(&*s, &UPSERT.replace("'b'", "'c'")).rows_affected, 2);
    assert_eq!(
        exec(
            &*s,
            "UPDATE wp_options SET option_value = 'c' WHERE option_name = 'a'"
        )
        .rows_affected,
        0
    );
    assert_eq!(
        exec(
            &*s,
            "UPDATE wp_options SET option_value = 'C' WHERE option_name = 'a'"
        )
        .rows_affected,
        1
    );
    assert_eq!(
        scalar(
            &*s,
            "SELECT option_value FROM wp_options WHERE option_name = 'a'"
        )
        .as_deref(),
        Some("C")
    );
    let ignored = exec(
        &*s,
        "INSERT IGNORE INTO wp_options (option_name, option_value) VALUES ('a', 'x')",
    );
    assert_eq!((ignored.rows_affected, ignored.insert_id), (0, None));
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn result_names_follow_mysql() {
    let s = store("names");
    exec(&*s, "INSERT INTO wp_posts (post_title) VALUES ('t')");
    assert_eq!(query(&*s, "SELECT * FROM wp_posts").columns[0].name, "ID");
    assert_eq!(query(&*s, "SELECT id FROM wp_posts").columns[0].name, "id");
    let count = query(&*s, "SELECT COUNT(*) FROM wp_posts WHERE ID = 0");
    assert_eq!(count.columns[0].name, "COUNT(*)");
    assert_eq!(count.rows[0].values()[0], DbValue::Int(0));
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn zero_dates_round_trip() {
    let s = store("zero_dates");
    exec(
        &*s,
        "INSERT INTO wp_posts (post_title, post_date_gmt) VALUES ('a', '2024-01-01 00:00:00')",
    );
    exec(
        &*s,
        "INSERT INTO wp_posts (post_title, post_date_gmt) VALUES ('b', '0000-00-00 00:00:00')",
    );
    assert_eq!(
        scalar(
            &*s,
            "SELECT post_date_gmt FROM wp_posts ORDER BY post_date_gmt"
        )
        .as_deref(),
        Some("0000-00-00 00:00:00")
    );
    assert_eq!(
        query(
            &*s,
            "SELECT ID FROM wp_posts WHERE post_date_gmt = '0000-00-00 00:00:00'"
        )
        .rows
        .len(),
        1
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn case_insensitive_text() {
    let s = store("ci");
    exec(
        &*s,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('Hello', 'v')",
    );
    let dup = s.execute(
        &parse_one("INSERT INTO wp_options (option_name, option_value) VALUES ('hello', 'v')")
            .unwrap(),
    );
    assert!(matches!(dup, Err(DbError::Constraint { .. })), "{dup:?}");
    assert_eq!(
        query(
            &*s,
            "SELECT option_id FROM wp_options WHERE option_name = 'HÉLLO'"
        )
        .rows
        .len(),
        1
    );
    assert_eq!(
        query(
            &*s,
            "SELECT option_id FROM wp_options WHERE option_name LIKE 'HEL%'"
        )
        .rows
        .len(),
        1
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn explicit_id_bumps_identity() {
    let s = store("identity");
    exec(
        &*s,
        "INSERT INTO wp_posts (ID, post_title) VALUES (500, 'a')",
    );
    assert_eq!(
        exec(&*s, "INSERT INTO wp_posts (post_title) VALUES ('b')").insert_id,
        Some(501)
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn delete_expired_transients_runs() {
    let s = store("transients");
    exec(
        &*s,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('_transient_x', 'a'), ('_transient_timeout_x', '100'), ('_transient_y', 'b'), ('_transient_timeout_y', '9999999999')",
    );
    let r = exec(
        &*s,
        r"DELETE a, b FROM wp_options a, wp_options b WHERE a.option_name LIKE '\\_transient\\_%' AND a.option_name NOT LIKE '\\_transient\\_timeout\\_%' AND b.option_name = CONCAT('_transient_timeout_', SUBSTRING(a.option_name, 12)) AND b.option_value < 1700000000",
    );
    assert_eq!(r.rows_affected, 2);
    assert_eq!(
        scalar(&*s, "SELECT COUNT(*) FROM wp_options").as_deref(),
        Some("2")
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn error_inside_transaction_does_not_abort_it() {
    let s = store("savepoints");
    exec(&*s, "START TRANSACTION");
    exec(&*s, "INSERT INTO wp_options (option_name) VALUES ('p')");
    assert!(matches!(
        s.execute(&parse_one("INSERT INTO wp_options (option_name) VALUES ('p')").unwrap()),
        Err(DbError::Constraint { .. })
    ));
    exec(&*s, "INSERT INTO wp_options (option_name) VALUES ('q')");
    exec(&*s, "COMMIT");
    assert_eq!(
        scalar(&*s, "SELECT COUNT(*) FROM wp_options").as_deref(),
        Some("2")
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn replace_is_atomic() {
    let s = store("replace");
    exec(
        &*s,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('a', 'old')",
    );
    assert!(
        s.execute(
            &parse_one("REPLACE INTO wp_options (option_name, option_value) VALUES ('a', NULL)")
                .unwrap()
        )
        .is_err()
    );
    assert_eq!(
        scalar(
            &*s,
            "SELECT option_value FROM wp_options WHERE option_name = 'a'"
        )
        .as_deref(),
        Some("old")
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn mysqli_values() {
    let s = store("values");
    let row = query(&*s, "SELECT 1 = 1, CAST('1.5' AS DECIMAL(10,2)), NULL");
    assert_eq!(
        row.rows[0].values(),
        [DbValue::Int(1), DbValue::Text("1.50".into()), DbValue::Null]
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn found_rows_and_show() {
    let s = store("found_rows");
    for i in 0..3 {
        exec(
            &*s,
            &format!("INSERT INTO wp_posts (post_title) VALUES ('p{i}')"),
        );
    }
    assert_eq!(
        query(&*s, "SELECT SQL_CALC_FOUND_ROWS ID FROM wp_posts LIMIT 1")
            .rows
            .len(),
        1
    );
    assert_eq!(scalar(&*s, "SELECT FOUND_ROWS()").as_deref(), Some("3"));
    let columns = query(&*s, "SHOW FULL COLUMNS FROM wp_options");
    let id = &columns.rows[0];
    assert_eq!(
        (
            id.get("Type").cloned(),
            id.get("Key").cloned(),
            id.get("Extra").cloned()
        ),
        (
            Some(DbValue::Text("bigint unsigned".into())),
            Some(DbValue::Text("PRI".into())),
            Some(DbValue::Text("auto_increment".into()))
        )
    );
    let id = exec(&*s, "INSERT INTO wp_posts (post_title) VALUES ('q')")
        .insert_id
        .unwrap();
    assert_eq!(scalar(&*s, "SELECT LAST_INSERT_ID()"), Some(id.to_string()));
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn reset_rolls_back_and_clears() {
    let s = store("reset");
    exec(&*s, "START TRANSACTION");
    exec(&*s, "INSERT INTO wp_options (option_name) VALUES ('a')");
    s.reset().unwrap();
    assert!(!s.in_transaction());
    assert_eq!(
        scalar(&*s, "SELECT COUNT(*) FROM wp_options").as_deref(),
        Some("0")
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn implicit_default_applies() {
    let s = store("implicit_default");
    exec(&*s, "INSERT INTO wp_options (option_name) VALUES ('a')");
    assert_eq!(
        scalar(
            &*s,
            "SELECT option_value FROM wp_options WHERE option_name = 'a'"
        )
        .as_deref(),
        Some("")
    );
}
