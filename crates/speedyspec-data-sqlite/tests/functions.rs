use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use rusqlite::types::Value;
use speedyspec_data_sqlite::register;
use speedyspec_data_translate::SessionVars;

fn connection() -> (Connection, Arc<Mutex<SessionVars>>) {
    let conn = Connection::open_in_memory().unwrap();
    let session = Arc::new(Mutex::new(SessionVars::mysql_defaults(
        "wordpress",
        "8.4.0",
    )));
    register(&conn, session.clone()).unwrap();
    (conn, session)
}

fn one(conn: &Connection, sql: &str) -> Value {
    conn.query_row(sql, [], |r| r.get::<_, Value>(0))
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

fn check(conn: &Connection, cases: &[(&str, Value)]) {
    for (sql, expected) in cases {
        assert_eq!(&one(conn, &format!("SELECT {sql}")), expected, "{sql}");
    }
}

#[test]
fn sqlite_is_new_enough() {
    let (conn, _) = connection();
    let Value::Text(version) = one(&conn, "SELECT sqlite_version()") else {
        panic!()
    };
    let parts: Vec<u32> = version.split('.').map(|p| p.parse().unwrap()).collect();
    assert!(parts >= vec![3, 45, 0], "{version}");
}

#[test]
fn strings() {
    let (conn, _) = connection();
    check(
        &conn,
        &[
            ("concat('a', NULL)", Value::Null),
            ("concat('a', 'b')", text("ab")),
            ("length('é')", Value::Integer(2)),
            ("char_length('é')", Value::Integer(1)),
            ("lower('ÉCOLE')", text("école")),
            ("upper('école')", text("ÉCOLE")),
            ("substring('hello', 0)", text("")),
            ("substring('hello', -3)", text("llo")),
            ("substring('hello', 2, 3)", text("ell")),
            ("mid('hello', 2, 2)", text("el")),
            ("left('hello', 2)", text("he")),
            ("right('hello', 2)", text("lo")),
            ("substring_index('a.b.c', '.', 2)", text("a.b")),
            ("substring_index('a.b.c', '.', -1)", text("c")),
            ("locate('b', 'abc')", Value::Integer(2)),
            ("locate('z', 'abc')", Value::Integer(0)),
            ("field('b', 'a', 'b', 'c')", Value::Integer(2)),
            ("field('z', 'a')", Value::Integer(0)),
            ("md5('a')", text("0cc175b9c0f1b6a831c399e269772661")),
        ],
    );
}

#[test]
fn like_and_collation() {
    let (conn, _) = connection();
    check(
        &conn,
        &[
            ("'ÉCOLE' LIKE 'ecole'", Value::Integer(1)),
            (r"'a_c' LIKE 'a\_c' ESCAPE '\'", Value::Integer(1)),
            (r"'abc' LIKE 'a\_c' ESCAPE '\'", Value::Integer(0)),
            ("CAST('ABC' AS BLOB) LIKE 'abc'", Value::Integer(0)),
            ("'Hello' REGEXP '^h'", Value::Integer(1)),
            ("'Hello' = 'hello' COLLATE speedyspec_ci", Value::Integer(1)),
            ("'a ' = 'a' COLLATE speedyspec_ci", Value::Integer(1)),
        ],
    );
    let mut stmt = conn
        .prepare("SELECT column1 FROM (VALUES ('banana'), ('Apple'), ('cherry')) ORDER BY column1 COLLATE speedyspec_ci")
        .unwrap();
    let sorted: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(sorted, ["Apple", "banana", "cherry"]);
}

#[test]
fn dates() {
    let (conn, _) = connection();
    check(
        &conn,
        &[
            (
                "speedyspec_date_add('2024-01-31', 1, 'MONTH')",
                text("2024-02-29"),
            ),
            (
                "speedyspec_date_sub('2024-03-01 00:00:00', 1, 'SECOND')",
                text("2024-02-29 23:59:59"),
            ),
            (
                "speedyspec_date_add('2024-01-01', '1:30', 'HOUR_MINUTE')",
                text("2024-01-01 01:30:00"),
            ),
            (
                "speedyspec_date_add('2024-01-01 10:00:00', 2, 'DAY')",
                text("2024-01-03 10:00:00"),
            ),
            (
                "speedyspec_date_sub('2024-03-31', 1, 'MONTH')",
                text("2024-02-29"),
            ),
            (
                "date_format('2024-03-05 14:07:09', '%Y-%m-%d %H:%i:%s %W %M %e %c %j')",
                text("2024-03-05 14:07:09 Tuesday March 5 3 065"),
            ),
            ("date_format('0000-00-00 00:00:00', '%Y')", Value::Null),
            (
                "unix_timestamp('1970-01-02 00:00:00')",
                Value::Integer(86400),
            ),
            ("from_unixtime(86400)", text("1970-01-02 00:00:00")),
            ("unix_timestamp('0000-00-00 00:00:00')", Value::Integer(0)),
            ("year('0000-00-00 00:00:00')", Value::Integer(0)),
            ("year('2024-03-05')", Value::Integer(2024)),
            ("month('2024-03-05 01:02:03')", Value::Integer(3)),
            ("hour('2024-03-05 14:07:09')", Value::Integer(14)),
            ("date('2024-03-05 14:07:09')", text("2024-03-05")),
            ("date('0000-00-00 00:00:00')", text("0000-00-00")),
            ("dayofweek('2024-03-05')", Value::Integer(3)),
            ("weekday('2024-03-05')", Value::Integer(1)),
            ("dayofyear('2024-03-05')", Value::Integer(65)),
            ("quarter('2024-03-05')", Value::Integer(1)),
            ("last_day('2024-02-10')", text("2024-02-29")),
            ("datediff('2024-03-05', '2024-03-01')", Value::Integer(4)),
            ("week('2024-03-05', 1)", Value::Integer(10)),
            ("week('2024-01-01', 0)", Value::Integer(0)),
            ("week('2024-12-31', 3)", Value::Integer(1)),
        ],
    );
}

#[test]
fn other() {
    let (conn, _) = connection();
    check(
        &conn,
        &[
            ("greatest(1, 5, 3)", Value::Integer(5)),
            ("greatest(1, NULL)", Value::Null),
            ("least('b', 'a')", text("a")),
            ("if(1 > 0, 'y', 'n')", text("y")),
            ("if(0, 'y', 'n')", text("n")),
            // MySQL's ISNULL(x) reaches SQLite as `x IS NULL` (the DML pass): ISNULL is SQLite's postfix keyword.
            ("NULL IS NULL", Value::Integer(1)),
            ("inet_aton('10.0.0.1')", Value::Integer(167772161)),
            ("inet_ntoa(167772161)", text("10.0.0.1")),
            ("get_lock('x', 10)", Value::Integer(1)),
            ("release_lock('x')", Value::Integer(1)),
        ],
    );
    let Value::Real(r) = one(&conn, "SELECT rand()") else {
        panic!()
    };
    assert!((0.0..1.0).contains(&r));
    let Value::Text(uuid) = one(&conn, "SELECT uuid()") else {
        panic!()
    };
    assert_eq!(uuid.len(), 36);
}

#[test]
fn session() {
    let (conn, session) = connection();
    let default = session.lock().unwrap().get("sql_mode").unwrap().to_string();
    assert_eq!(
        one(&conn, "SELECT speedyspec_sysvar('sql_mode')"),
        Value::Text(default)
    );
    session.lock().unwrap().set("sql_mode", "X");
    assert_eq!(
        one(&conn, "SELECT speedyspec_sysvar('sql_mode')"),
        text("X")
    );
    assert_eq!(one(&conn, "SELECT speedyspec_sysvar('nope')"), Value::Null);
    assert_eq!(one(&conn, "SELECT version()"), text("8.4.0"));
    assert_eq!(one(&conn, "SELECT database()"), text("wordpress"));
}
