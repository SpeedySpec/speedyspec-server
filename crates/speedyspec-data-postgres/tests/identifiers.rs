//! Identifiers, result names and placeholders as PostgreSQL needs them, named as MySQL names them.

mod common;

use common::{assert_pg, wp_dialect};
use speedyspec_data::DbValue;
use speedyspec_data_postgres::RowCount;

fn none(mysql: &str, pg: &str) {
    assert_pg(&wp_dialect(), mysql, vec![], &[pg]);
}

#[test]
fn quotes_and_keeps_stored_case() {
    none(
        "SELECT `option_value` FROM `wp_options` WHERE `option_name` = 'siteurl' LIMIT 1",
        r#"SELECT "option_value" FROM "wp_options" WHERE "option_name" = 'siteurl' LIMIT 1"#,
    );
}

#[test]
fn resolves_case_and_names_as_written() {
    none(
        "SELECT id, post_title FROM wp_posts WHERE Post_Status = 'publish'",
        r#"SELECT "ID" AS "id", "post_title" FROM "wp_posts" WHERE "post_status" = 'publish'"#,
    );
    none("SELECT ID FROM wp_posts", r#"SELECT "ID" FROM "wp_posts""#);
    none("SELECT * FROM wp_posts", r#"SELECT * FROM "wp_posts""#);
}

#[test]
fn qualified_and_aliased() {
    none(
        "SELECT p.id FROM wp_posts AS p ORDER BY P.post_date DESC",
        r#"SELECT "p"."ID" AS "id" FROM "wp_posts" AS "p" ORDER BY "p"."post_date" DESC"#,
    );
}

#[test]
fn select_alias_reference() {
    none(
        "SELECT option_name AS Name FROM wp_options ORDER BY name",
        r#"SELECT "option_name" AS "Name" FROM "wp_options" ORDER BY "Name""#,
    );
}

#[test]
fn unknown_column_kept() {
    none(
        "SELECT nope FROM wp_options",
        r#"SELECT "nope" FROM "wp_options""#,
    );
}

#[test]
fn expression_names_pinned() {
    none(
        "SELECT COUNT(*) FROM wp_posts",
        r#"SELECT COUNT(*) AS "COUNT(*)" FROM "wp_posts""#,
    );
}

const IN_QUERY: &str =
    "SELECT option_value FROM wp_options WHERE option_name IN (?, ?) AND autoload = ? LIMIT 2, 5";

#[test]
fn numbered_placeholders() {
    let params = vec![
        DbValue::Text("a".into()),
        DbValue::Text("b".into()),
        DbValue::Text("on".into()),
    ];
    let emitted = assert_pg(
        &wp_dialect(),
        IN_QUERY,
        params.clone(),
        &[
            r#"SELECT "option_value" FROM "wp_options" WHERE "option_name" IN ($1, $2) AND "autoload" = $3 LIMIT 5 OFFSET 2"#,
        ],
    );
    assert_eq!(emitted[0].sql.params, params);
}

#[test]
fn plan_counts() {
    let params = vec![
        DbValue::Text("a".into()),
        DbValue::Text("b".into()),
        DbValue::Text("on".into()),
    ];
    let select = assert_pg(
        &wp_dialect(),
        IN_QUERY,
        params,
        &[
            r#"SELECT "option_value" FROM "wp_options" WHERE "option_name" IN ($1, $2) AND "autoload" = $3 LIMIT 5 OFFSET 2"#,
        ],
    );
    assert_eq!(select[0].count, RowCount::Rows);
    let delete = assert_pg(
        &wp_dialect(),
        "DELETE FROM wp_options WHERE option_name = 'a'",
        vec![],
        &[r#"DELETE FROM "wp_options" WHERE "option_name" = 'a'"#],
    );
    assert_eq!(delete[0].count, RowCount::Affected);
}
