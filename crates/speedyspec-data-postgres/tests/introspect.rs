//! The connection's setup, the support functions and the introspector, on a live PostgreSQL.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{WP_OPTIONS, WP_POSTMETA, WP_POSTS, pg_url};
use speedyspec_data::{DbError, Introspector};
use speedyspec_data_postgres::__test::{Connection, check_server_version, execute, introspector};
use speedyspec_data_postgres::{PgPlan, PostgresDialect};
use speedyspec_data_query::parse_one;
use speedyspec_data_translate::catalogue_ddl;

const LIVE: &str = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL";

/// A fresh schema `t_<name>` with the catalogue and `ddl` in it.
fn schema(name: &str, ddl: &[&str]) -> Rc<RefCell<Connection>> {
    let mut admin = postgres::Client::connect(&pg_url(), postgres::NoTls).unwrap();
    admin
        .batch_execute(&format!("DROP SCHEMA IF EXISTS t_{name} CASCADE"))
        .unwrap();
    let mut conn = Connection::new("test", &pg_url(), &format!("t_{name}")).unwrap();
    let dialect = PostgresDialect::new(None);
    let statements: Vec<speedyspec_data::Statement> = catalogue_ddl()
        .into_iter()
        .chain(ddl.iter().map(|s| parse_one(s).unwrap()))
        .collect();
    for stmt in statements {
        let PgPlan::Sql(emitted) = dialect.plan(&stmt).unwrap() else {
            panic!()
        };
        for e in emitted {
            execute(&mut conn, &e.sql).unwrap_or_else(|err| panic!("{}: {err:?}", e.sql.sql));
        }
    }
    Rc::new(RefCell::new(conn))
}

fn scalar(conn: &Rc<RefCell<Connection>>, sql: &str) -> String {
    let mut c = conn.borrow_mut();
    let row = c
        .client()
        .unwrap()
        .query_one(&format!("SELECT ({sql})::text"), &[])
        .unwrap();
    row.get::<_, Option<String>>(0)
        .unwrap_or_else(|| "NULL".into())
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn support_functions_are_lenient() {
    let conn = schema("support", &[]);
    assert_eq!(scalar(&conn, "speedyspec_signed('12abc')"), "12");
    assert_eq!(scalar(&conn, "speedyspec_signed('')"), "0");
    assert_eq!(scalar(&conn, "speedyspec_signed(NULL)"), "NULL");
    assert_eq!(scalar(&conn, "speedyspec_double('1.5e3x')"), "1500");
    assert_eq!(scalar(&conn, "speedyspec_date('2024-02-30')"), "NULL");
    assert_eq!(scalar(&conn, "speedyspec_decimal(' -3.25 ')"), "-3.25");
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn ci_collation_behaves_like_520_ci() {
    let conn = schema("collation", &[]);
    assert_eq!(
        scalar(&conn, "'Hello' = 'hello' COLLATE speedyspec_ci"),
        "true"
    );
    assert_eq!(
        scalar(&conn, "'élan' = 'ELAN' COLLATE speedyspec_ci"),
        "true"
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn wp_options_round_trip() {
    let conn = schema("options", &[WP_OPTIONS]);
    let i = introspector(conn);
    let columns = i.columns("wp_options").unwrap();
    let id = &columns[0];
    assert_eq!(
        (
            id.name.as_str(),
            id.sql_type.as_str(),
            id.key.as_str(),
            id.extra.as_str()
        ),
        ("option_id", "bigint unsigned", "PRI", "auto_increment")
    );
    let name = &columns[1];
    assert_eq!(
        (
            name.sql_type.as_str(),
            name.key.as_str(),
            name.collation.as_deref()
        ),
        ("varchar(191)", "UNI", Some("utf8mb4_unicode_520_ci"))
    );
    assert_eq!(columns[2].sql_type, "longtext");
    assert_eq!(
        (
            columns[3].sql_type.as_str(),
            columns[3].key.as_str(),
            columns[3].default.as_deref()
        ),
        ("varchar(20)", "MUL", Some("yes"))
    );
    let names: Vec<(String, bool)> = i
        .indexes("wp_options")
        .unwrap()
        .into_iter()
        .map(|x| (x.name, x.unique))
        .collect();
    assert_eq!(
        names,
        [
            ("PRIMARY".into(), true),
            ("option_name".into(), true),
            ("autoload".into(), false)
        ]
    );
    assert_eq!(
        i.table_collation("wp_options").unwrap().as_deref(),
        Some("utf8mb4_unicode_520_ci")
    );
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn same_index_name_in_two_tables() {
    let conn = schema(
        "two_names",
        &[
            "CREATE TABLE a_one (id int, name varchar(20), KEY name (name))",
            "CREATE TABLE a_two (id int, name varchar(20), KEY name (name))",
        ],
    );
    let i = introspector(conn);
    for table in ["a_one", "a_two"] {
        assert_eq!(i.indexes(table).unwrap()[0].name, "name");
    }
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn prefix_length_reported() {
    let i = introspector(schema("prefix", &[WP_POSTMETA]));
    let meta_key = i
        .indexes("wp_postmeta")
        .unwrap()
        .into_iter()
        .find(|x| x.name == "meta_key")
        .unwrap();
    assert_eq!(meta_key.columns, [("meta_key".to_string(), Some(191))]);
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn zero_date_default_reported() {
    let i = introspector(schema("zero_date", &[WP_POSTS]));
    let date = i
        .columns("wp_posts")
        .unwrap()
        .into_iter()
        .find(|c| c.name == "post_date")
        .unwrap();
    assert_eq!(date.default.as_deref(), Some("0000-00-00 00:00:00"));
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn server_version_shape() {
    let i = introspector(schema("version", &[]));
    assert!(
        i.server_version()
            .unwrap()
            .starts_with("8.4.0-speedyspec-postgres-")
    );
}

#[test]
fn refuses_old_server() {
    assert!(matches!(
        check_server_version(150_004, "15.4"),
        Err(DbError::Connection { .. })
    ));
    assert!(check_server_version(160_000, "16.0").is_ok());
    let _ = LIVE;
}
