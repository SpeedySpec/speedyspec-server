//! The SQLite store answering as MySQL does: counts, insert ids, found rows, results, the schema and transactions.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use speedyspec_data::{
    DbError, DbValue, EmulatedOp, EngineConfig, ExecResult, Rows, SqlStore, Statement,
};
use speedyspec_data_query::parse_one;
use speedyspec_data_sqlite::SqliteStore;
use speedyspec_data_translate::testing::StaticIntrospector;
use speedyspec_data_translate::{SessionVars, answer_emulated};

const WP_OPTIONS: &str = "CREATE TABLE wp_options ( option_id bigint(20) unsigned NOT NULL auto_increment, option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY  (option_id), UNIQUE KEY option_name (option_name), KEY autoload (autoload) ) DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_520_ci";
const WP_POSTMETA: &str = "CREATE TABLE wp_postmeta ( meta_id bigint(20) unsigned NOT NULL auto_increment, post_id bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, meta_value longtext, PRIMARY KEY  (meta_id), KEY post_id (post_id), KEY meta_key (meta_key(191)) ) DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_520_ci";
const WP_POSTS: &str = "CREATE TABLE wp_posts ( ID bigint(20) unsigned NOT NULL auto_increment, post_title text NOT NULL, post_type varchar(20) NOT NULL default 'post', PRIMARY KEY  (ID) ) DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_520_ci";
const UPSERT: &str = "INSERT INTO wp_options (option_name, option_value, autoload) VALUES ('n', 'v', 'auto') ON DUPLICATE KEY UPDATE option_name = VALUES(option_name), option_value = VALUES(option_value), autoload = VALUES(autoload)";

fn config(url: &str, options: &[(&str, &str)]) -> EngineConfig {
    EngineConfig {
        name: "sqlite".into(),
        url: url.into(),
        options: options
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn memory() -> SqliteStore {
    let store =
        SqliteStore::open(&config("sqlite://:memory:", &[("database", "wordpress")])).unwrap();
    for ddl in [WP_OPTIONS, WP_POSTMETA] {
        exec(&store, ddl);
    }
    store
}

fn stmt(sql: &str) -> Statement {
    parse_one(sql).unwrap_or_else(|e| panic!("{sql}: {e:?}"))
}

fn exec(store: &SqliteStore, sql: &str) -> ExecResult {
    store
        .execute(&stmt(sql))
        .unwrap_or_else(|e| panic!("{sql}: {e:?}"))
}

fn query(store: &SqliteStore, sql: &str) -> Rows {
    store
        .query(&stmt(sql))
        .unwrap_or_else(|e| panic!("{sql}: {e:?}"))
}

fn scalar(store: &SqliteStore, sql: &str) -> Option<String> {
    let rows = query(store, sql);
    rows.rows
        .first()
        .and_then(|r| r.get_index(0))
        .and_then(DbValue::to_mysql_text)
        .map(|t| String::from_utf8(t).unwrap())
}

fn t(s: &str) -> DbValue {
    DbValue::Text(s.into())
}

#[test]
fn changed_rows_like_mysql() {
    let store = memory();
    assert_eq!(
        exec(
            &store,
            "INSERT INTO wp_options (option_name, option_value) VALUES ('a', 'x')"
        )
        .rows_affected,
        1
    );
    assert_eq!(
        exec(
            &store,
            "UPDATE wp_options SET option_value = 'x' WHERE option_name = 'a'"
        )
        .rows_affected,
        0
    );
    assert_eq!(
        exec(
            &store,
            "UPDATE wp_options SET option_value = 'Hello' WHERE option_name = 'a'"
        )
        .rows_affected,
        1
    );
    assert_eq!(
        exec(
            &store,
            "UPDATE wp_options SET option_value = 'hello' WHERE option_name = 'a'"
        )
        .rows_affected,
        1
    );
    assert_eq!(
        scalar(
            &store,
            "SELECT option_value FROM wp_options WHERE option_name = 'a'"
        )
        .as_deref(),
        Some("hello")
    );
    assert_eq!(exec(&store, UPSERT).rows_affected, 1);
    assert_eq!(exec(&store, UPSERT).rows_affected, 0);
    assert_eq!(exec(&store, &UPSERT.replace("'v'", "'w'")).rows_affected, 2);
    assert_eq!(
        exec(
            &store,
            "REPLACE INTO wp_options (option_name, option_value, autoload) VALUES ('a', 'r', 'no')"
        )
        .rows_affected,
        2
    );
    assert_eq!(
        exec(
            &store,
            "INSERT IGNORE INTO wp_options (option_name, option_value) VALUES ('a', 'i')"
        )
        .rows_affected,
        0
    );
}

#[test]
fn insert_ids() {
    let store = memory();
    assert_eq!(
        exec(&store, "INSERT INTO wp_options (option_name) VALUES ('a')").insert_id,
        Some(1)
    );
    assert_eq!(
        exec(
            &store,
            "INSERT INTO wp_options (option_name) VALUES ('b'), ('c')"
        )
        .insert_id,
        Some(2)
    );
    exec(&store, "CREATE TABLE n (a int)");
    assert_eq!(exec(&store, "INSERT INTO n (a) VALUES (1)").insert_id, None);
    assert_eq!(
        exec(
            &store,
            "INSERT IGNORE INTO wp_options (option_name) VALUES ('a')"
        )
        .insert_id,
        None
    );
}

#[test]
fn found_rows() {
    let store = memory();
    exec(&store, WP_POSTS);
    for i in 0..15 {
        exec(
            &store,
            &format!("INSERT INTO wp_posts (post_title) VALUES ('p{i}')"),
        );
    }
    let page = query(
        &store,
        "SELECT SQL_CALC_FOUND_ROWS wp_posts.ID FROM wp_posts WHERE 1=1 AND post_type = 'post' ORDER BY wp_posts.ID DESC LIMIT 0, 10",
    );
    assert_eq!(page.rows.len(), 10);
    assert_eq!(scalar(&store, "SELECT FOUND_ROWS()").as_deref(), Some("15"));
    query(&store, "SELECT * FROM wp_posts LIMIT 3");
    assert_eq!(scalar(&store, "SELECT FOUND_ROWS()").as_deref(), Some("3"));
    store.reset().unwrap();
    assert_eq!(scalar(&store, "SELECT FOUND_ROWS()").as_deref(), Some("0"));
}

#[test]
fn results_stringified_like_mysql() {
    let store = memory();
    exec(&store, "CREATE TABLE d (p decimal(10,2), f double, z int)");
    exec(&store, "INSERT INTO d (p, f, z) VALUES (5, 1.5, NULL)");
    let rows = query(&store, "SELECT p, f, z FROM d");
    assert_eq!(rows.rows[0].values()[0], t("5.00"));
    assert_eq!(
        rows.rows[0].values()[1].to_mysql_text(),
        Some(b"1.5".to_vec())
    );
    assert_eq!(rows.rows[0].values()[2].to_mysql_text(), None);
    exec(&store, "INSERT INTO wp_options (option_name) VALUES ('a')");
    let named = query(&store, "SELECT option_name AS n FROM wp_options o");
    let column = &named.columns[0];
    assert_eq!(
        (
            column.name.as_str(),
            column.org_name.as_str(),
            column.org_table.as_str()
        ),
        ("n", "option_name", "wp_options")
    );
}

#[test]
fn implicit_default_applies() {
    let store = memory();
    exec(&store, "INSERT INTO wp_options (option_name) VALUES ('a')");
    assert_eq!(
        scalar(
            &store,
            "SELECT option_value FROM wp_options WHERE option_name = 'a'"
        )
        .as_deref(),
        Some("")
    );
}

#[test]
fn like_escape() {
    let store = memory();
    exec(
        &store,
        "INSERT INTO wp_options (option_name) VALUES ('wp_a'), ('wpxa')",
    );
    let rows = query(
        &store,
        r"SELECT option_name FROM wp_options WHERE option_name LIKE 'wp\\_%'",
    );
    let names: Vec<DbValue> = rows.rows.iter().map(|r| r.values()[0].clone()).collect();
    assert_eq!(names, [t("wp_a")]);
}

#[test]
fn unique_is_case_insensitive() {
    let store = memory();
    exec(
        &store,
        "INSERT INTO wp_options (option_name) VALUES ('Key')",
    );
    let err = store
        .execute(&stmt("INSERT INTO wp_options (option_name) VALUES ('key')"))
        .unwrap_err();
    assert!(matches!(err, DbError::Constraint { .. }), "{err:?}");
}

#[test]
fn dbdelta_round_trip() {
    let store = memory();
    let schema = StaticIntrospector::from_mysql_ddl(&[WP_OPTIONS, WP_POSTMETA]);
    let mysql = |op: EmulatedOp| -> Rows {
        answer_emulated(
            &op,
            &schema,
            &mut SessionVars::mysql_defaults("wordpress", "8.4.0"),
            0,
        )
        .unwrap()
    };
    let values = |rows: &Rows| -> Vec<Vec<DbValue>> {
        rows.rows.iter().map(|r| r.values().to_vec()).collect()
    };
    let describe = query(&store, "DESCRIBE wp_postmeta");
    assert_eq!(
        values(&describe),
        values(&mysql(EmulatedOp::ShowColumns {
            table: "wp_postmeta".into(),
            full: false,
            like: None
        }))
    );
    let index = query(&store, "SHOW INDEX FROM wp_postmeta");
    assert_eq!(
        values(&index),
        values(&mysql(EmulatedOp::ShowIndex {
            table: "wp_postmeta".into()
        }))
    );
    let create = query(&store, "SHOW CREATE TABLE wp_options");
    assert_eq!(
        values(&create),
        values(&mysql(EmulatedOp::ShowCreateTable {
            table: "wp_options".into()
        }))
    );
}

#[test]
fn emulated_admin() {
    let store = memory();
    let tables = query(&store, "SHOW TABLES");
    let names: Vec<String> = tables
        .rows
        .iter()
        .map(|r| String::from_utf8(r.values()[0].to_mysql_text().unwrap()).unwrap())
        .collect();
    assert_eq!(names, ["wp_options", "wp_postmeta"]);
    assert!(
        scalar(&store, "SELECT VERSION()")
            .unwrap()
            .starts_with("8.4.0")
    );
    assert_eq!(
        scalar(&store, "SELECT DATABASE()").as_deref(),
        Some("wordpress")
    );
    let default = scalar(&store, "SELECT @@SESSION.sql_mode");
    exec(&store, "SET SESSION sql_mode = 'NO_ENGINE_SUBSTITUTION'");
    assert_eq!(
        scalar(&store, "SELECT @@SESSION.sql_mode").as_deref(),
        Some("NO_ENGINE_SUBSTITUTION")
    );
    store.reset().unwrap();
    assert_eq!(scalar(&store, "SELECT @@SESSION.sql_mode"), default);
}

#[test]
fn information_schema_tables() {
    let store = memory();
    let rows = query(
        &store,
        "SELECT table_name FROM information_schema.tables WHERE table_schema = DATABASE() ORDER BY table_name",
    );
    assert_eq!(rows.columns[0].name, "TABLE_NAME");
    let names: Vec<DbValue> = rows.rows.iter().map(|r| r.values()[0].clone()).collect();
    assert_eq!(names, [t("wp_options"), t("wp_postmeta")]);
}

#[test]
fn transients_cleanup() {
    let store = memory();
    exec(
        &store,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('_transient_a', 'x'), ('_transient_timeout_a', '999'), ('_transient_b', 'y'), ('_transient_timeout_b', '99999999999')",
    );
    let result = exec(
        &store,
        r"DELETE a, b FROM wp_options a, wp_options b WHERE a.option_name LIKE '\\_transient\\_%' AND a.option_name NOT LIKE '\\_transient\\_timeout\\_%' AND b.option_name = CONCAT('_transient_timeout_', SUBSTRING(a.option_name, 12)) AND b.option_value < 1700000000",
    );
    assert_eq!(result.rows_affected, 2);
    assert_eq!(
        scalar(&store, "SELECT COUNT(*) FROM wp_options").as_deref(),
        Some("2")
    );
    assert!(
        scalar(
            &store,
            "SELECT option_value FROM wp_options WHERE option_name = '_transient_b'"
        )
        .is_some()
    );
}

#[test]
fn statement_is_atomic() {
    let store = memory();
    exec(
        &store,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('a', 'old')",
    );
    let failed = store.execute(&stmt(
        "REPLACE INTO wp_options (option_name, option_value) VALUES ('a', NULL)",
    ));
    assert!(failed.is_err(), "{failed:?}");
    assert_eq!(
        scalar(
            &store,
            "SELECT option_value FROM wp_options WHERE option_name = 'a'"
        )
        .as_deref(),
        Some("old")
    );
}

#[test]
fn concurrent_writer_waits() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}", dir.path().join("site.db").display());
    let store = SqliteStore::open(&config(&url, &[])).unwrap();
    exec(&store, WP_OPTIONS);
    let writer = |url: String| {
        std::thread::spawn(move || {
            let other = SqliteStore::open(&config(&url, &[])).unwrap();
            other.begin().unwrap();
            other
                .execute(&stmt(
                    "INSERT INTO wp_options (option_name) VALUES ('other')",
                ))
                .unwrap();
            std::thread::sleep(Duration::from_millis(200));
            other.commit().unwrap();
        })
    };
    let thread = writer(url.clone());
    std::thread::sleep(Duration::from_millis(50));
    let start = Instant::now();
    exec(
        &store,
        "INSERT INTO wp_options (option_name) VALUES ('mine')",
    );
    assert!(
        start.elapsed() >= Duration::from_millis(150),
        "{:?}",
        start.elapsed()
    );
    thread.join().unwrap();

    exec(&store, "DELETE FROM wp_options");
    let impatient = SqliteStore::open(&config(&url, &[("busy_timeout_ms", "50")])).unwrap();
    let thread = writer(url.clone());
    std::thread::sleep(Duration::from_millis(50));
    let err = impatient
        .execute(&stmt(
            "INSERT INTO wp_options (option_name) VALUES ('mine')",
        ))
        .unwrap_err();
    assert!(
        matches!(&err, DbError::Engine { code: Some(c), .. } if c == "SQLITE_BUSY"),
        "{err:?}"
    );
    thread.join().unwrap();
}

#[test]
fn reset_rolls_back_raw_transaction() {
    let store = memory();
    exec(&store, "START TRANSACTION");
    exec(&store, "INSERT INTO wp_options (option_name) VALUES ('a')");
    store.reset().unwrap();
    assert!(!store.in_transaction());
    assert_eq!(
        scalar(&store, "SELECT COUNT(*) FROM wp_options").as_deref(),
        Some("0")
    );
}
