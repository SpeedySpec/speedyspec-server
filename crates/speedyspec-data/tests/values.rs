use std::rc::Rc;

use sqlparser::dialect::MySqlDialect;
use sqlparser::parser::Parser;
use speedyspec_data::{DbError, DbValue, Row, Statement, StatementKind, TxControl};

#[test]
fn to_mysql_text_matches_mysqli() {
    assert_eq!(DbValue::Null.to_mysql_text(), None);
    assert_eq!(DbValue::Int(-5).to_mysql_text(), Some(b"-5".to_vec()));
    assert_eq!(
        DbValue::UInt(18446744073709551615).to_mysql_text(),
        Some(b"18446744073709551615".to_vec())
    );
    assert_eq!(DbValue::Float(0.1).to_mysql_text(), Some(b"0.1".to_vec()));
    assert_eq!(
        DbValue::Text("é".into()).to_mysql_text(),
        Some("é".as_bytes().to_vec())
    );
}

#[test]
fn row_get_returns_last_duplicate_column() {
    let columns: Rc<[String]> = Rc::from(vec!["id".to_string(), "id".to_string()]);
    let row = Row::new(columns, vec![DbValue::Int(1), DbValue::Int(2)]);
    assert_eq!(row.get("id"), Some(&DbValue::Int(2)));
    assert_eq!(row.get_index(0), Some(&DbValue::Int(1)));
    assert_eq!(row.get("missing"), None);
}

fn kind_of(sql: &str) -> StatementKind {
    let mut trees = Parser::parse_sql(&MySqlDialect {}, sql).expect(sql);
    Statement::new(trees.remove(0), vec![]).kind()
}

#[test]
fn statement_kind_from_tree() {
    assert_eq!(kind_of("SELECT 1"), StatementKind::Select);
    assert_eq!(kind_of("INSERT INTO t VALUES (1)"), StatementKind::Insert);
    assert_eq!(kind_of("REPLACE INTO t VALUES (1)"), StatementKind::Replace);
    assert_eq!(kind_of("UPDATE t SET a = 1"), StatementKind::Update);
    assert_eq!(kind_of("DELETE FROM t"), StatementKind::Delete);
    assert_eq!(kind_of("CREATE TABLE t (a int)"), StatementKind::Ddl);
    assert_eq!(kind_of("SHOW TABLES"), StatementKind::Show);
    assert_eq!(kind_of("SET NAMES utf8mb4"), StatementKind::Set);
    assert_eq!(
        kind_of("START TRANSACTION"),
        StatementKind::Transaction(TxControl::Begin)
    );
    assert_eq!(
        kind_of("COMMIT"),
        StatementKind::Transaction(TxControl::Commit)
    );
}

#[test]
fn db_error_display_names_engine() {
    let e = DbError::Engine {
        engine: "default".into(),
        code: Some("1062".into()),
        message: "Duplicate".into(),
    };
    let text = e.to_string();
    assert!(text.contains("default") && text.contains("1062") && text.contains("Duplicate"));
}
