mod support;

use speedyspec_data::DbError;
use speedyspec_data_query::parse_one;
use speedyspec_data_translate::{ExpressionPass, HintsPass, LexicalPass, number_placeholders};
use support::*;

fn check(pass: impl speedyspec_data_query::RewritePass + 'static, sql: &str, out: &str) {
    assert_sql(&statement_of(rewrite_one(pass, sql, &cx()).unwrap()), out);
}

#[test]
fn lexical_backticks_and_strings() {
    check(
        LexicalPass,
        r#"SELECT `option_value` FROM `wp_options` WHERE `option_name` = "siteurl" LIMIT 1"#,
        r#"SELECT "option_value" FROM "wp_options" WHERE "option_name" = 'siteurl' LIMIT 1"#,
    );
}

#[test]
fn lexical_limit_comma() {
    check(
        LexicalPass,
        "SELECT ID FROM wp_posts ORDER BY post_date DESC LIMIT 20, 10",
        "SELECT ID FROM wp_posts ORDER BY post_date DESC LIMIT 10 OFFSET 20",
    );
}

#[test]
fn lexical_null_safe_and_pipes() {
    check(
        LexicalPass,
        "SELECT * FROM t WHERE a <=> NULL OR b = 1 || c = 2",
        "SELECT * FROM t WHERE a IS NOT DISTINCT FROM NULL OR b = 1 OR c = 2",
    );
}

#[test]
fn lexical_like_gets_mysql_escape() {
    check(
        LexicalPass,
        r"SELECT option_name FROM wp_options WHERE option_name LIKE '\\_transient\\_%'",
        r"SELECT option_name FROM wp_options WHERE option_name LIKE '\_transient\_%' ESCAPE '\'",
    );
    check(
        LexicalPass,
        r"SELECT a FROM t WHERE a NOT LIKE 'x%'",
        r"SELECT a FROM t WHERE a NOT LIKE 'x%' ESCAPE '\'",
    );
}

#[test]
fn lexical_explicit_escape_kept() {
    check(
        LexicalPass,
        "SELECT a FROM t WHERE a LIKE 'x|_%' ESCAPE '|'",
        "SELECT a FROM t WHERE a LIKE 'x|_%' ESCAPE '|'",
    );
}

#[test]
fn lexical_string_escapes() {
    check(
        LexicalPass,
        r#"SELECT 'it\'s', "a\"b""#,
        r#"SELECT 'it''s', 'a"b'"#,
    );
}

#[test]
fn hints_dropped() {
    check(
        HintsPass,
        "SELECT HIGH_PRIORITY SQL_NO_CACHE ID FROM wp_posts FORCE INDEX (type_status_date) WHERE post_type = 'post'",
        "SELECT ID FROM wp_posts WHERE post_type = 'post'",
    );
    check(
        HintsPass,
        "SELECT a FROM t STRAIGHT_JOIN u ON t.id = u.id",
        "SELECT a FROM t JOIN u ON t.id = u.id",
    );
    // The generic dialect reads SQL_CALC_FOUND_ROWS as a column, so this one compares printed text.
    let kept =
        statement_of(rewrite_one(HintsPass, "SELECT SQL_CALC_FOUND_ROWS a FROM t", &cx()).unwrap());
    assert_eq!(
        kept.tree().to_string(),
        "SELECT SQL_CALC_FOUND_ROWS a FROM t"
    );
    check(
        HintsPass,
        "INSERT DELAYED INTO t (a) VALUES (1)",
        "INSERT INTO t (a) VALUES (1)",
    );
}

#[test]
fn expressions_casts() {
    check(
        ExpressionPass,
        "SELECT CAST(meta_value AS SIGNED), CAST(meta_value AS UNSIGNED), CAST(ID AS CHAR) FROM wp_postmeta",
        "SELECT CAST(meta_value AS BIGINT), CAST(meta_value AS BIGINT), CAST(ID AS TEXT) FROM wp_postmeta",
    );
}

#[test]
fn expressions_intervals() {
    check(
        ExpressionPass,
        "SELECT * FROM wp_posts WHERE post_date > DATE_SUB(NOW(), INTERVAL 7 DAY) AND post_modified < post_date + INTERVAL ? MONTH",
        "SELECT * FROM wp_posts WHERE post_date > speedyspec_date_sub(NOW(), 7, 'DAY') AND post_modified < speedyspec_date_add(post_date, ?, 'MONTH')",
    );
}

#[test]
fn expressions_compound_interval() {
    // sqlparser 0.64 rejects MySQL's compound interval units: a parser gap (plan 1 Task 3's rule), listed in the PR.
    assert!(matches!(
        parse_one("SELECT DATE_ADD(d, INTERVAL '1:30' HOUR_MINUTE) FROM t"),
        Err(DbError::Parse { .. })
    ));
}

#[test]
fn expressions_sysvar_alias() {
    check(
        ExpressionPass,
        "SELECT @@SESSION.sql_mode",
        r#"SELECT speedyspec_sysvar('sql_mode') AS "@@SESSION.sql_mode""#,
    );
    check(
        ExpressionPass,
        "SELECT @@sql_mode AS m",
        "SELECT speedyspec_sysvar('sql_mode') AS m",
    );
}

fn numbered(sql: &str) -> String {
    let mut stmt = parse_one(sql).unwrap();
    number_placeholders(&mut stmt);
    stmt.tree().to_string()
}

#[test]
fn number_placeholders_in_text_order() {
    for (sql, count) in [
        (
            "UPDATE wp_posts p JOIN wp_postmeta m ON m.meta_key = ? SET p.post_status = ? WHERE m.meta_value = ? LIMIT ?",
            4,
        ),
        (
            "INSERT INTO t (a, b) VALUES (?, ?), (?, ?) ON DUPLICATE KEY UPDATE b = ?",
            5,
        ),
        (
            "SELECT * FROM t WHERE a IN (SELECT b FROM u WHERE c = ?) AND d = ? LIMIT ?",
            3,
        ),
    ] {
        let text = numbered(sql);
        let found: Vec<usize> = text
            .match_indices('?')
            .map(|(i, _)| {
                text[i + 1..]
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap()
            })
            .collect();
        assert_eq!(found, (1..=count).collect::<Vec<_>>(), "{text}");
    }
}

#[test]
fn number_placeholders_is_idempotent() {
    let mut stmt = parse_one("SELECT * FROM t WHERE a = ? AND b = ?").unwrap();
    number_placeholders(&mut stmt);
    let once = stmt.tree().to_string();
    number_placeholders(&mut stmt);
    assert_eq!(stmt.tree().to_string(), once);
}
