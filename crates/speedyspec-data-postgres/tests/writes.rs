//! Writes with MySQL's counts: upserts, REPLACE, insert ids, changed-rows guards, LIMIT and multi-table DELETE.

mod common;

use common::{assert_pg, wp_dialect};
use speedyspec_data::{DbError, DbValue, DialectKind};
use speedyspec_data_postgres::RowCount;
use speedyspec_data_query::parse_one;

fn t(s: &str) -> DbValue {
    DbValue::Text(s.into())
}

#[test]
fn add_option_upsert() {
    let e = assert_pg(
        &wp_dialect(),
        "INSERT INTO wp_options (option_name, option_value, autoload) VALUES ('a', 'b', 'auto') ON DUPLICATE KEY UPDATE option_name = VALUES(option_name), option_value = VALUES(option_value), autoload = VALUES(autoload)",
        vec![],
        &[
            r#"INSERT INTO "wp_options" ("option_name", "option_value", "autoload") VALUES ('a', 'b', 'auto') ON CONFLICT ("option_name") DO UPDATE SET "option_name" = "excluded"."option_name", "option_value" = "excluded"."option_value", "autoload" = "excluded"."autoload" WHERE ("wp_options"."option_name" COLLATE "C", "wp_options"."option_value" COLLATE "C", "wp_options"."autoload" COLLATE "C") IS DISTINCT FROM ("excluded"."option_name" COLLATE "C", "excluded"."option_value" COLLATE "C", "excluded"."autoload" COLLATE "C") RETURNING "option_id" AS "__speedyspec_id", ("xmax" = 0) AS "__speedyspec_inserted""#,
        ],
    );
    assert_eq!(e[0].count, RowCount::Upsert);
}

#[test]
fn upsert_without_unique_is_insert() {
    assert_pg(
        &wp_dialect(),
        "INSERT INTO wp_postmeta (post_id, meta_key, meta_value) VALUES (1, 'k', 'v') ON DUPLICATE KEY UPDATE meta_value = VALUES(meta_value)",
        vec![],
        &[
            r#"INSERT INTO "wp_postmeta" ("post_id", "meta_key", "meta_value") VALUES (1, 'k', 'v') RETURNING "meta_id" AS "__speedyspec_id""#,
        ],
    );
}

#[test]
fn upsert_two_uniques_unsupported() {
    let sql = "INSERT INTO t_two_unique (id, name, n) VALUES (1, 'a', 1) ON DUPLICATE KEY UPDATE n = n + 1";
    match wp_dialect().plan(&parse_one(sql).unwrap()) {
        Err(DbError::Unsupported {
            construct,
            dialect: DialectKind::Postgres,
        }) => {
            assert!(
                construct.contains("PRIMARY") && construct.contains("name"),
                "{construct}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn insert_ignore() {
    assert_pg(
        &wp_dialect(),
        "INSERT IGNORE INTO wp_options (option_name, option_value, autoload) VALUES ('a', 'b', 'no')",
        vec![],
        &[
            r#"INSERT INTO "wp_options" ("option_name", "option_value", "autoload") VALUES ('a', 'b', 'no') ON CONFLICT DO NOTHING RETURNING "option_id" AS "__speedyspec_id""#,
        ],
    );
}

#[test]
fn insert_returns_id() {
    let e = assert_pg(
        &wp_dialect(),
        "INSERT INTO wp_posts (post_title) VALUES (?)",
        vec![t("x")],
        &[
            r#"INSERT INTO "wp_posts" ("post_title") VALUES ($1) RETURNING "ID" AS "__speedyspec_id""#,
        ],
    );
    assert_eq!(e[0].count, RowCount::Returning);
}

#[test]
fn explicit_id_moves_sequence() {
    let e = assert_pg(
        &wp_dialect(),
        "INSERT INTO wp_posts (ID, post_title) VALUES (500, 'x')",
        vec![],
        &[
            r#"INSERT INTO "wp_posts" ("ID", "post_title") VALUES (500, 'x') RETURNING "ID" AS "__speedyspec_id""#,
            r#"SELECT setval(pg_get_serial_sequence('"wp_posts"', 'ID'), GREATEST((SELECT COALESCE(MAX("ID"), 0) FROM "wp_posts"), COALESCE(pg_sequence_last_value(pg_get_serial_sequence('"wp_posts"', 'ID')), 0), 1)) AS "__speedyspec_sequence""#,
        ],
    );
    assert_eq!(e[1].count, RowCount::Hidden);
}

#[test]
fn replace_splits() {
    let e = assert_pg(
        &wp_dialect(),
        "REPLACE INTO wp_options (option_name, option_value, autoload) VALUES (?, ?, ?)",
        vec![t("a"), t("b"), t("yes")],
        &[
            r#"DELETE FROM "wp_options" WHERE "option_name" = $1"#,
            r#"INSERT INTO "wp_options" ("option_name", "option_value", "autoload") VALUES ($1, $2, $3) RETURNING "option_id" AS "__speedyspec_id""#,
        ],
    );
    assert_eq!(e[0].sql.params, [t("a")]);
    assert_eq!(e[1].sql.params, [t("a"), t("b"), t("yes")]);
}

#[test]
fn update_guard() {
    let e = assert_pg(
        &wp_dialect(),
        "UPDATE wp_options SET option_value = ? WHERE option_name = ?",
        vec![t("v"), t("n")],
        &[
            r#"UPDATE "wp_options" SET "option_value" = $1 WHERE ("option_name" = $2) AND "option_value" COLLATE "C" IS DISTINCT FROM $3 COLLATE "C""#,
        ],
    );
    assert_eq!(e[0].sql.params, [t("v"), t("n"), t("v")]);
    assert_pg(
        &wp_dialect(),
        "UPDATE wp_posts SET post_status = 'draft', menu_order = menu_order + 1 WHERE ID = 3",
        vec![],
        &[
            r#"UPDATE "wp_posts" SET "post_status" = 'draft', "menu_order" = "menu_order" + 1 WHERE ("ID" = 3) AND ("post_status" COLLATE "C", "menu_order") IS DISTINCT FROM ('draft' COLLATE "C", "menu_order" + 1)"#,
        ],
    );
}

#[test]
fn delete_limit() {
    assert_pg(
        &wp_dialect(),
        "DELETE FROM wp_options WHERE autoload = 'no' LIMIT 100",
        vec![],
        &[
            r#"DELETE FROM "wp_options" WHERE "ctid" IN (SELECT "ctid" FROM "wp_options" WHERE "autoload" = 'no' LIMIT 100)"#,
        ],
    );
}

#[test]
fn expired_transients() {
    let e = assert_pg(
        &wp_dialect(),
        r"DELETE a, b FROM wp_options a, wp_options b WHERE a.option_name LIKE '\\_transient\\_%' AND a.option_name NOT LIKE '\\_transient\\_timeout\\_%' AND b.option_name = CONCAT('_transient_timeout_', SUBSTRING(a.option_name, 12)) AND b.option_value < 1700000000",
        vec![],
        &[
            r#"WITH "__speedyspec_doomed" AS (SELECT "a"."ctid" AS "c0", "b"."ctid" AS "c1" FROM "wp_options" AS "a", "wp_options" AS "b" WHERE "a"."option_name" COLLATE "und-x-icu" ILIKE '\_transient\_%' ESCAPE '\' AND "a"."option_name" COLLATE "und-x-icu" NOT ILIKE '\_transient\_timeout\_%' ESCAPE '\' AND "b"."option_name" = ('_transient_timeout_' || SUBSTRING("a"."option_name", 12)::text) AND speedyspec_double("b"."option_value") < 1700000000), "__speedyspec_d0" AS (DELETE FROM "wp_options" WHERE "ctid" IN (SELECT "c0" FROM "__speedyspec_doomed" UNION SELECT "c1" FROM "__speedyspec_doomed") RETURNING 1) SELECT count(*) AS "__speedyspec_count" FROM "__speedyspec_d0""#,
        ],
    );
    assert_eq!(e[0].count, RowCount::CountColumn);
}

#[test]
fn update_guard_with_a_zero_date() {
    assert_pg(
        &wp_dialect(),
        "UPDATE wp_posts SET post_date_gmt = '0000-00-00 00:00:00', post_title = 't' WHERE ID = 1",
        vec![],
        &[
            r#"UPDATE "wp_posts" SET "post_date_gmt" = '-infinity', "post_title" = 't' WHERE ("ID" = 1) AND ("post_date_gmt", "post_title" COLLATE "C") IS DISTINCT FROM ('-infinity', 't' COLLATE "C")"#,
        ],
    );
}
