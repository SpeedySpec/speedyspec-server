//! MySQL functions, truthiness, casts, comparisons, collation and zero dates as PostgreSQL needs them.

mod common;

use common::{assert_pg, wp_dialect};
use speedyspec_data::{DbError, DbValue, DialectKind, Statement};
use speedyspec_data_query::parse_one;

/// `SELECT <mysql> FROM wp_posts` plans to `SELECT <pg> AS "<mysql>" FROM "wp_posts"`.
fn select(mysql: &str, pg: &str) {
    assert_pg(
        &wp_dialect(),
        &format!("SELECT {mysql} AS x FROM wp_posts"),
        vec![],
        &[&format!(r#"SELECT {pg} AS "x" FROM "wp_posts""#)],
    );
}

fn refused(sql: &str) -> String {
    match wp_dialect().plan(&parse_one(sql).unwrap()) {
        Err(DbError::Unsupported {
            construct,
            dialect: DialectKind::Postgres,
        }) => construct,
        other => panic!("{sql}: {other:?}"),
    }
}

#[test]
fn clock_functions() {
    select("NOW()", "LOCALTIMESTAMP(0)");
    select("CURRENT_TIMESTAMP", "LOCALTIMESTAMP(0)");
    select("CURDATE()", "CURRENT_DATE");
    select("CURTIME()", "LOCALTIME(0)");
    select(
        "UTC_TIMESTAMP()",
        "(now() AT TIME ZONE 'UTC')::timestamp(0)",
    );
    select(
        "UNIX_TIMESTAMP()",
        "floor(extract(epoch FROM now()))::bigint",
    );
    select(
        "UNIX_TIMESTAMP(post_date)",
        r#"floor(extract(epoch FROM "post_date"))::bigint"#,
    );
    select(
        "FROM_UNIXTIME(5)",
        "(to_timestamp(5) AT TIME ZONE 'UTC')::timestamp(0)",
    );
}

#[test]
fn date_parts() {
    select(
        "YEAR(post_date)",
        r#"extract(YEAR FROM "post_date")::integer"#,
    );
    select(
        "MONTH(post_date)",
        r#"extract(MONTH FROM "post_date")::integer"#,
    );
    select(
        "DAYOFMONTH(post_date)",
        r#"extract(DAY FROM "post_date")::integer"#,
    );
    select(
        "HOUR(post_date)",
        r#"extract(HOUR FROM "post_date")::integer"#,
    );
    select(
        "DAYOFWEEK(post_date)",
        r#"(extract(DOW FROM "post_date")::integer + 1)"#,
    );
    select(
        "WEEK(post_date)",
        r#"floor((extract(DOY FROM "post_date") + 6 - extract(DOW FROM "post_date")) / 7)::integer"#,
    );
    select(
        "WEEK(post_date, 3)",
        r#"extract(WEEK FROM "post_date")::integer"#,
    );
    select(
        "WEEK(post_date, 1)",
        r#"CASE WHEN extract(ISOYEAR FROM "post_date") < extract(YEAR FROM "post_date") THEN 0 WHEN extract(ISOYEAR FROM "post_date") > extract(YEAR FROM "post_date") THEN 53 ELSE extract(WEEK FROM "post_date")::integer END"#,
    );
    assert_eq!(
        refused("SELECT WEEK(post_date, 2) FROM wp_posts"),
        "WEEK mode 2"
    );
}

#[test]
fn date_sub_now() {
    assert_pg(
        &wp_dialect(),
        "SELECT ID FROM wp_posts WHERE post_date > DATE_SUB(NOW(), INTERVAL 7 DAY)",
        vec![],
        &[
            r#"SELECT "ID" FROM "wp_posts" WHERE "post_date" > (LOCALTIMESTAMP(0) - 7 * INTERVAL '1 day')"#,
        ],
    );
    select(
        "DATE_ADD(post_date, INTERVAL 2 HOUR)",
        r#"("post_date" + 2 * INTERVAL '1 hour')"#,
    );
}

#[test]
fn date_format_named() {
    assert_pg(
        &wp_dialect(),
        "SELECT DATE_FORMAT(post_date, '%Y-%m-%d') AS d FROM wp_posts",
        vec![],
        &[r#"SELECT to_char("post_date", 'YYYY-MM-DD') AS "d" FROM "wp_posts""#],
    );
    assert!(refused("SELECT DATE_FORMAT(post_date, '%U') FROM wp_posts").contains("DATE_FORMAT"));
}

#[test]
fn string_functions() {
    select(
        "CONCAT('a', post_title, ID)",
        r#"('a' || "post_title"::text || "ID"::text)"#,
    );
    select("CONCAT_WS(',', 'a', 'b')", "CONCAT_WS(',', 'a', 'b')");
    select("LENGTH(post_title)", r#"octet_length("post_title")"#);
    select("LCASE(post_title)", r#"lower("post_title")"#);
    select("UCASE(post_title)", r#"upper("post_title")"#);
    select(
        "LOCATE('a', post_title)",
        r#"position('a' IN "post_title")"#,
    );
    select("INSTR(post_title, 'a')", r#"strpos("post_title", 'a')"#);
    select(
        "SUBSTRING_INDEX(post_title, '/', 1)",
        r#"split_part("post_title", '/', 1)"#,
    );
    select(
        "SUBSTRING_INDEX(post_title, '/', -1)",
        r#"split_part("post_title", '/', -1)"#,
    );
    select(
        "SUBSTRING_INDEX(post_title, '/', 2)",
        r#"array_to_string((string_to_array("post_title", '/'))[1:2], '/')"#,
    );
    select(
        "SUBSTRING_INDEX(post_title, '/', -2)",
        r#"array_to_string((string_to_array("post_title", '/'))[greatest(cardinality(string_to_array("post_title", '/')) - 2 + 1, 1):], '/')"#,
    );
}

#[test]
fn aggregate_and_conditional_functions() {
    select(
        "GROUP_CONCAT(post_title ORDER BY ID SEPARATOR ';')",
        r#"string_agg("post_title"::text, ';' ORDER BY "ID")"#,
    );
    select(
        "GROUP_CONCAT(DISTINCT post_title)",
        r#"string_agg(DISTINCT "post_title"::text, ',')"#,
    );
    select(
        "IF(menu_order, 'a', 'b')",
        r#"CASE WHEN "menu_order"::numeric <> 0 THEN 'a' ELSE 'b' END"#,
    );
    select(
        "FIELD(post_status, 'a', 'b')",
        r#"CASE "post_status" WHEN 'a' THEN 1 WHEN 'b' THEN 2 ELSE 0 END"#,
    );
    select(
        "GREATEST(ID, menu_order)",
        r#"CASE WHEN "ID" IS NULL OR "menu_order" IS NULL THEN NULL ELSE GREATEST("ID", "menu_order") END"#,
    );
    select("RAND()", "random()");
    assert_eq!(refused("SELECT RAND(5) FROM wp_posts"), "RAND(seed)");
    select("ISNULL(post_title)", r#""post_title" IS NULL"#);
    select("LAST_INSERT_ID()", "lastval()");
    select("UUID()", "gen_random_uuid()::text");
    select("ID DIV 2", r#"trunc("ID" / 2)::bigint"#);
}

#[test]
fn casts() {
    select(
        "CAST(post_title AS UNSIGNED)",
        r#"speedyspec_signed("post_title"::text)"#,
    );
    select(
        "CAST(post_title AS DECIMAL(10,2))",
        r#"speedyspec_decimal("post_title"::text)::numeric(10,2)"#,
    );
    select("CAST(ID AS CHAR)", r#""ID"::text"#);
    select(
        "CAST(post_title AS DATE)",
        r#"speedyspec_date("post_title"::text)"#,
    );
    select(
        "CAST(post_title AS DATETIME)",
        r#"speedyspec_datetime("post_title"::text)"#,
    );
    select(
        "CAST(post_title AS BINARY)",
        r#"convert_to("post_title"::text, 'UTF8')"#,
    );
}

#[test]
fn meta_cast() {
    assert_pg(
        &wp_dialect(),
        "SELECT post_id FROM wp_postmeta WHERE CAST(meta_value AS SIGNED) > 5",
        vec![],
        &[r#"SELECT "post_id" FROM "wp_postmeta" WHERE speedyspec_signed("meta_value"::text) > 5"#],
    );
}

#[test]
fn text_number_compare() {
    let d = wp_dialect();
    assert_pg(
        &d,
        "SELECT option_name FROM wp_options WHERE option_value < 1700000000",
        vec![],
        &[
            r#"SELECT "option_name" FROM "wp_options" WHERE speedyspec_double("option_value") < 1700000000"#,
        ],
    );
    assert_pg(
        &d,
        "SELECT option_name FROM wp_options WHERE option_value < ?",
        vec![DbValue::Int(1_700_000_000)],
        &[r#"SELECT "option_name" FROM "wp_options" WHERE speedyspec_double("option_value") < $1"#],
    );
    assert_pg(
        &d,
        "SELECT option_name FROM wp_options WHERE option_value < ?",
        vec![DbValue::Text("1".into())],
        &[r#"SELECT "option_name" FROM "wp_options" WHERE "option_value" < $1"#],
    );
}

#[test]
fn truthiness() {
    assert_pg(
        &wp_dialect(),
        "SELECT ID FROM wp_posts WHERE 1 AND menu_order",
        vec![],
        &[r#"SELECT "ID" FROM "wp_posts" WHERE true AND "menu_order"::numeric <> 0"#],
    );
}

#[test]
fn like_and_regexp() {
    let d = wp_dialect();
    assert_pg(
        &d,
        r"SELECT option_name FROM wp_options WHERE option_name LIKE '\\_transient\\_%' AND option_name NOT REGEXP '^_site'",
        vec![],
        &[
            r#"SELECT "option_name" FROM "wp_options" WHERE "option_name" COLLATE "und-x-icu" ILIKE '\_transient\_%' ESCAPE '\' AND "option_name" COLLATE "und-x-icu" !~* '^_site'"#,
        ],
    );
    assert_pg(
        &d,
        "SELECT option_name FROM wp_options WHERE BINARY option_name LIKE 'A%'",
        vec![],
        &[
            r#"SELECT "option_name" FROM "wp_options" WHERE "option_name" COLLATE "C" LIKE 'A%' ESCAPE '\'"#,
        ],
    );
}

#[test]
fn zero_dates() {
    let d = wp_dialect();
    assert_pg(
        &d,
        "SELECT ID FROM wp_posts WHERE post_date_gmt = '0000-00-00 00:00:00'",
        vec![],
        &[r#"SELECT "ID" FROM "wp_posts" WHERE "post_date_gmt" = '-infinity'"#],
    );
    let emitted = assert_pg(
        &d,
        "SELECT ID FROM wp_posts WHERE post_date_gmt = ?",
        vec![DbValue::Text("0000-00-00 00:00:00".into())],
        &[r#"SELECT "ID" FROM "wp_posts" WHERE "post_date_gmt" = $1"#],
    );
    assert_eq!(emitted[0].sql.params, [DbValue::Text("-infinity".into())]);
    assert_pg(
        &d,
        "SELECT ID FROM wp_posts WHERE post_title = '0000-00-00 00:00:00'",
        vec![],
        &[r#"SELECT "ID" FROM "wp_posts" WHERE "post_title" = '0000-00-00 00:00:00'"#],
    );
}

#[test]
fn match_against_unsupported() {
    assert_eq!(
        refused("SELECT ID FROM wp_posts WHERE MATCH (post_title) AGAINST ('x')"),
        "MATCH ... AGAINST"
    );
}

#[allow(dead_code)]
fn _unused(_: Statement) {}
