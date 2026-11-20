use speedyspec_data::{DbError, StatementKind};
use speedyspec_data_query::{parse_many, parse_one};

#[test]
fn parses_wordpress_core_queries() {
    let cases = [
        (
            "SELECT option_name, option_value FROM wp_options WHERE autoload IN ('yes','on','auto-on','auto')",
            StatementKind::Select,
        ),
        (
            "INSERT INTO `wp_options` (`option_name`, `option_value`, `autoload`) VALUES ('a', 'b', 'auto') ON DUPLICATE KEY UPDATE `option_name` = VALUES(`option_name`), `option_value` = VALUES(`option_value`), `autoload` = VALUES(`autoload`)",
            StatementKind::Insert,
        ),
        (
            "SELECT SQL_CALC_FOUND_ROWS wp_posts.ID FROM wp_posts WHERE 1=1 ORDER BY wp_posts.post_date DESC LIMIT 0, 10",
            StatementKind::Select,
        ),
        ("SELECT FOUND_ROWS()", StatementKind::Select),
        ("SHOW FULL COLUMNS FROM `wp_posts` ", StatementKind::Show),
        (
            "SET NAMES 'utf8mb4' COLLATE 'utf8mb4_unicode_520_ci'",
            StatementKind::Set,
        ),
    ];
    for (sql, kind) in cases {
        let stmt = parse_one(sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
        assert_eq!(stmt.kind(), kind, "{sql}");
        assert!(stmt.params().is_empty());
    }
}

#[test]
fn parse_error_has_position() {
    match parse_one("SELECT * FROM WHERE") {
        Err(DbError::Parse { line, column, .. }) => {
            assert_eq!(line, 1);
            assert!(column > 0);
        }
        other => panic!("expected a parse error, got {other:?}"),
    }
}

#[test]
fn rejects_empty_and_multiple() {
    for sql in ["", "-- c"] {
        assert!(
            matches!(parse_one(sql), Err(DbError::Parse { ref message, .. }) if message == "empty query"),
            "{sql:?}"
        );
    }
    assert!(matches!(
        parse_one("SELECT 1; SELECT 2"),
        Err(DbError::Parse { ref message, .. }) if message == "multiple statements in one query"
    ));
    assert_eq!(parse_many("SELECT 1; SELECT 2").unwrap().len(), 2);
}

#[test]
fn errors_at_end_of_input_have_position() {
    match parse_one("SELECT *\nFROM t WHERE") {
        Err(DbError::Parse { line, column, .. }) => assert_eq!((line, column), (2, 13)),
        other => panic!("{other:?}"),
    }
    match parse_one("SELECT 1; SELECT 2") {
        Err(DbError::Parse { line, column, .. }) => assert_eq!((line, column), (1, 11)),
        other => panic!("{other:?}"),
    }
}
