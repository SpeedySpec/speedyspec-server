//! The final review's findings, each pinned with the inputs that broke on a live server.

mod common;

use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use common::{WP_OPTIONS, WP_POSTMETA, WP_POSTS, pg_url};
use speedyspec_data::{DbError, DbValue, EngineConfig, ExecResult, Rows, SqlStore, Statement};
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
    for ddl in [WP_OPTIONS, WP_POSTS, WP_POSTMETA] {
        exec(&*store, ddl);
    }
    store
}

fn stmt(sql: &str, params: Vec<DbValue>) -> Statement {
    let (tree, _) = parse_one(sql)
        .unwrap_or_else(|e| panic!("{sql}: {e:?}"))
        .into_parts();
    Statement::new(tree, params)
}

fn try_exec(store: &dyn SqlStore, sql: &str) -> Result<ExecResult, DbError> {
    store.execute(&stmt(sql, vec![]))
}

fn exec(store: &dyn SqlStore, sql: &str) -> ExecResult {
    try_exec(store, sql).unwrap_or_else(|e| panic!("{sql}: {e:?}"))
}

fn query(store: &dyn SqlStore, sql: &str) -> Rows {
    store
        .query(&stmt(sql, vec![]))
        .unwrap_or_else(|e| panic!("{sql}: {e:?}"))
}

fn text(v: Option<&DbValue>) -> Option<String> {
    v.and_then(DbValue::to_mysql_text)
        .map(|t| String::from_utf8(t).unwrap())
}

fn scalar(store: &dyn SqlStore, sql: &str) -> Option<String> {
    text(query(store, sql).rows.first().and_then(|r| r.get_index(0)))
}

fn table(store: &dyn SqlStore, sql: &str) -> Vec<Vec<Option<String>>> {
    let rows = query(store, sql);
    rows.rows
        .iter()
        .map(|r| {
            (0..rows.columns.len())
                .map(|i| text(r.get_index(i)))
                .collect()
        })
        .collect()
}

fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

fn code(result: &Result<impl std::fmt::Debug, DbError>) -> Option<String> {
    match result {
        Err(DbError::Engine { code, .. }) => code.clone(),
        _ => None,
    }
}

const IGNORE: &str = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL";

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn guard_takes_non_text_values() {
    let _ = IGNORE;
    let st = store("rf_guard");
    let st = &*st;
    exec(
        st,
        "INSERT INTO wp_postmeta (post_id, meta_key, meta_value) VALUES (1, 'views', '1')",
    );
    let meta = "SELECT meta_value FROM wp_postmeta WHERE meta_key = 'views'";
    let set = |value: &str| {
        exec(
            st,
            &format!("UPDATE wp_postmeta SET meta_value = {value} WHERE meta_key = 'views'"),
        )
        .rows_affected
    };
    assert_eq!(set("meta_value + 1"), 1);
    assert_eq!(scalar(st, meta), s("2"));
    assert_eq!(set("5"), 1);
    assert_eq!(set("5"), 0);
    assert_eq!(scalar(st, meta), s("5"));
    assert_eq!(set("5.5"), 1);
    assert_eq!(scalar(st, meta), s("5.5"));
    assert_eq!(set("NOW()"), 1);
    exec(
        st,
        "INSERT INTO wp_posts (post_title, menu_order) VALUES ('t', 3)",
    );
    assert_eq!(
        exec(st, "UPDATE wp_posts SET post_title = menu_order").rows_affected,
        1
    );
    assert_eq!(scalar(st, "SELECT post_title FROM wp_posts"), s("3"));
    exec(
        st,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('n', '1')",
    );
    let upsert = exec(
        st,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('n', 'x') ON DUPLICATE KEY UPDATE option_value = 5",
    );
    assert_eq!(upsert.rows_affected, 2);
    assert_eq!(
        scalar(
            st,
            "SELECT option_value FROM wp_options WHERE option_name = 'n'"
        ),
        s("5")
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn upsert_counter_idioms() {
    let st = store("rf_counter");
    let st = &*st;
    exec(
        st,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('c', '1')",
    );
    let value = "SELECT option_value FROM wp_options WHERE option_name = 'c'";
    let n = exec(
        st,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('c', '1') ON DUPLICATE KEY UPDATE option_value = option_value + 1",
    );
    assert_eq!(n.rows_affected, 2);
    assert_eq!(scalar(st, value), s("2"));
    exec(
        st,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('c', '7') ON DUPLICATE KEY UPDATE option_value = VALUES(option_value) + 1",
    );
    assert_eq!(scalar(st, value), s("8"));
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn group_by_alias_and_ordinal() {
    let st = store("rf_group");
    let st = &*st;
    exec(
        st,
        "INSERT INTO wp_posts (post_title, post_status) VALUES ('a', 'publish'), ('b', 'publish'), ('c', 'draft')",
    );
    let expected = vec![vec![s("draft"), s("1")], vec![s("publish"), s("2")]];
    assert_eq!(
        table(
            st,
            "SELECT post_status AS s, COUNT(*) FROM wp_posts GROUP BY s ORDER BY s"
        ),
        expected
    );
    assert_eq!(
        table(
            st,
            "SELECT post_status, COUNT(*) FROM wp_posts GROUP BY 1 ORDER BY 1"
        ),
        expected
    );
    assert_eq!(
        table(
            st,
            "SELECT LEFT(post_title, 1) AS l, COUNT(*) FROM wp_posts GROUP BY l ORDER BY l"
        )
        .len(),
        3
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn division_as_mysql() {
    let st = store("rf_division");
    let st = &*st;
    exec(
        st,
        "INSERT INTO wp_posts (post_title, menu_order) VALUES ('a', 3)",
    );
    assert_eq!(scalar(st, "SELECT 7/2"), s("3.5000"));
    assert_eq!(scalar(st, "SELECT 1/2"), s("0.5000"));
    assert_eq!(scalar(st, "SELECT menu_order/2 FROM wp_posts"), s("1.5000"));
    assert_eq!(scalar(st, "SELECT 7 DIV 2"), s("3"));
    assert_eq!(scalar(st, "SELECT 1/0"), None);
    assert_eq!(scalar(st, "SELECT 5 DIV 0"), None);
    assert_eq!(scalar(st, "SELECT 5 % 0"), None);
    assert_eq!(scalar(st, "SELECT COUNT(ID)/0 FROM wp_posts"), None);
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn zero_dates_in_functions() {
    let st = store("rf_zero_fn");
    let st = &*st;
    exec(
        st,
        "INSERT INTO wp_posts (post_title, post_date, post_status) VALUES ('d', '2024-01-02 03:04:05', 'draft')",
    );
    let one = |e: &str| scalar(st, &format!("SELECT {e} FROM wp_posts"));
    assert_eq!(one("YEAR(post_date_gmt)"), s("0"));
    assert_eq!(one("MONTH(post_date_gmt)"), s("0"));
    assert_eq!(one("DAYOFMONTH(post_date_gmt)"), s("0"));
    assert_eq!(one("UNIX_TIMESTAMP(post_date_gmt)"), s("0"));
    assert_eq!(
        one("DATE_FORMAT(post_date_gmt, '%Y-%m-%d')"),
        s("0000-00-00")
    );
    assert_eq!(one("CAST(post_date_gmt AS CHAR)"), s("0000-00-00 00:00:00"));
    assert_eq!(one("CONCAT(post_date_gmt, '')"), s("0000-00-00 00:00:00"));
    assert_eq!(one("CAST(post_date AS CHAR)"), s("2024-01-02 03:04:05"));
    assert_eq!(one("YEAR(post_date)"), s("2024"));
    assert_eq!(
        scalar(
            st,
            "SELECT COUNT(*) FROM wp_posts WHERE YEAR(post_date_gmt) = 2024"
        ),
        s("0")
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn autocommit_off_survives_ddl() {
    let st = store("rf_autocommit");
    let st = &*st;
    exec(st, "SET autocommit = 0");
    exec(st, "CREATE TABLE wp_k (id int)");
    assert!(st.in_transaction());
    exec(
        st,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('ac', 'v')",
    );
    exec(st, "ROLLBACK");
    assert_eq!(
        scalar(
            st,
            "SELECT COUNT(*) FROM wp_options WHERE option_name = 'ac'"
        ),
        s("0")
    );
}

fn kill_backend(st: &dyn SqlStore) {
    let pid = scalar(st, "SELECT pg_backend_pid()").unwrap();
    let mut admin = postgres::Client::connect(&pg_url(), postgres::NoTls).unwrap();
    admin
        .batch_execute(&format!("SELECT pg_terminate_backend({pid})"))
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn connection_loss_in_transaction_resets_the_session() {
    let st = store("rf_lost_tx");
    let st = &*st;
    exec(st, "SET SESSION sql_mode = 'NO_ZERO_DATE'");
    exec(st, "START TRANSACTION");
    exec(
        st,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('a', '1')",
    );
    kill_backend(st);
    let lost = try_exec(
        st,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('b', '1')",
    );
    assert!(matches!(lost, Err(DbError::Connection { .. })), "{lost:?}");
    assert!(!st.in_transaction());
    let next = exec(
        st,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('c', '1')",
    );
    assert!(next.insert_id.is_some());
    assert_eq!(scalar(st, "SELECT @@SESSION.sql_mode"), s("NO_ZERO_DATE"));
    assert_eq!(scalar(st, "SELECT COUNT(*) FROM wp_options"), s("1"));
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn connection_loss_does_not_poison_the_schema_cache() {
    let st = store("rf_lost_cache");
    let st = &*st;
    kill_backend(st);
    let first = exec(
        st,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('a', '1')",
    );
    assert!(first.insert_id.is_some(), "{first:?}");
    let second = exec(
        st,
        "INSERT INTO wp_options (option_name, option_value) VALUES ('b', '1')",
    );
    assert!(second.insert_id.is_some(), "{second:?}");
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn null_or_zero_auto_increment_generates() {
    let st = store("rf_auto");
    let st = &*st;
    let a = exec(
        st,
        "INSERT INTO wp_posts (ID, post_title) VALUES (NULL, 'n')",
    );
    assert_eq!(a.insert_id, Some(1));
    let b = exec(st, "INSERT INTO wp_posts (ID, post_title) VALUES (0, 'z')");
    assert_eq!(b.insert_id, Some(2));
    let c = st
        .execute(&stmt(
            "INSERT INTO wp_posts (ID, post_title) VALUES (?, ?)",
            vec![DbValue::Int(0), DbValue::Text("p".into())],
        ))
        .unwrap();
    assert_eq!(c.insert_id, Some(3));
    assert_eq!(
        scalar(st, "SELECT post_title FROM wp_posts WHERE ID = 3"),
        s("p")
    );
    assert_eq!(
        scalar(st, "SELECT COUNT(*) FROM wp_posts WHERE ID = 0"),
        s("0")
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn bad_argument_counts_are_errors() {
    let st = store("rf_arity");
    let st = &*st;
    let concat = st.query(&stmt("SELECT CONCAT()", vec![]));
    assert_eq!(code(&concat).as_deref(), Some("1582"), "{concat:?}");
    let field = st.query(&stmt("SELECT FIELD(ID) FROM wp_posts", vec![]));
    assert_eq!(code(&field).as_deref(), Some("1582"), "{field:?}");
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn simple_case_compares_its_operand() {
    let st = store("rf_case");
    let st = &*st;
    exec(st, "INSERT INTO wp_posts (post_title) VALUES ('T'), ('U')");
    assert_eq!(
        table(
            st,
            "SELECT CASE post_title WHEN 'T' THEN 1 ELSE 0 END FROM wp_posts ORDER BY ID"
        ),
        vec![vec![s("1")], vec![s("0")]]
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn long_index_names_are_shortened() {
    let st = store("rf_index");
    let st = &*st;
    exec(
        st,
        "CREATE TABLE wp_woocommerce_downloadable_product_permissions (permission_id bigint unsigned NOT NULL auto_increment, user_id bigint unsigned NOT NULL default 0, order_id bigint unsigned NOT NULL default 0, downloads_remaining varchar(9) NULL, access_expires datetime NULL, PRIMARY KEY (permission_id), KEY user_order_remaining_expires (user_id, order_id, downloads_remaining, access_expires))",
    );
    let names = table(
        st,
        "SHOW INDEX FROM wp_woocommerce_downloadable_product_permissions",
    );
    assert!(
        names
            .iter()
            .any(|r| r.contains(&s("user_order_remaining_expires"))),
        "{names:?}"
    );
    exec(
        st,
        "ALTER TABLE wp_woocommerce_downloadable_product_permissions DROP INDEX user_order_remaining_expires",
    );
    exec(
        st,
        "CREATE INDEX user_order_remaining_expires ON wp_woocommerce_downloadable_product_permissions (user_id)",
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn update_join_runs() {
    let st = store("rf_update_join");
    let st = &*st;
    exec(st, "INSERT INTO wp_posts (post_title) VALUES ('a'), ('b')");
    exec(
        st,
        "INSERT INTO wp_postmeta (post_id, meta_key, meta_value) VALUES (1, 'k', 'v')",
    );
    let sql = "UPDATE wp_posts p JOIN wp_postmeta m ON m.post_id = p.ID SET p.post_title = 'j' WHERE m.meta_key = 'k'";
    assert_eq!(exec(st, sql).rows_affected, 1);
    assert_eq!(exec(st, sql).rows_affected, 0);
    assert_eq!(
        table(st, "SELECT post_title FROM wp_posts ORDER BY ID"),
        vec![vec![s("j")], vec![s("b")]]
    );
    let left = try_exec(
        st,
        "UPDATE wp_posts p LEFT JOIN wp_postmeta m ON m.post_id = p.ID SET p.post_title = 'x' WHERE m.meta_id IS NULL",
    );
    assert!(matches!(left, Err(DbError::Unsupported { .. })), "{left:?}");
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn empty_string_into_integer_is_zero() {
    let st = store("rf_empty_int");
    let st = &*st;
    exec(
        st,
        "INSERT INTO wp_posts (post_title, menu_order) VALUES ('e', '')",
    );
    assert_eq!(scalar(st, "SELECT menu_order FROM wp_posts"), s("0"));
    exec(st, "UPDATE wp_posts SET menu_order = 4");
    exec(st, "UPDATE wp_posts SET menu_order = ''");
    assert_eq!(scalar(st, "SELECT menu_order FROM wp_posts"), s("0"));
    st.execute(&stmt(
        "INSERT INTO wp_posts (post_title, menu_order) VALUES (?, ?)",
        vec![DbValue::Text("p".into()), DbValue::Text(String::new())],
    ))
    .unwrap();
}
