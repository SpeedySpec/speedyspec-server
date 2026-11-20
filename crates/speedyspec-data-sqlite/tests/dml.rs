//! MySQL DML as SQLite runs it: changed-row counts, upserts, REPLACE, multi-table statements, functions and
//! comparisons.

mod support;

use speedyspec_data::{DbError, DbValue, DialectKind};
use speedyspec_data_sqlite::{CountRule, InsertIdRule, SqlitePlan, SqliteStep};
use support::{assert_sqlite_sql, plan_error, plan_with};

const WP_OPTIONS: &str = "CREATE TABLE wp_options ( option_id bigint(20) unsigned NOT NULL auto_increment, option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY  (option_id), UNIQUE KEY option_name (option_name), KEY autoload (autoload) )";
const WP_POSTS: &str = "CREATE TABLE wp_posts ( ID bigint(20) unsigned NOT NULL auto_increment, post_author bigint(20) unsigned NOT NULL default '0', post_date datetime NOT NULL default '0000-00-00 00:00:00', post_title text NOT NULL, post_status varchar(20) NOT NULL default 'publish', post_name varchar(200) NOT NULL default '', post_type varchar(20) NOT NULL default 'post', PRIMARY KEY  (ID) )";
const WP_POSTMETA: &str = "CREATE TABLE wp_postmeta ( meta_id bigint(20) unsigned NOT NULL auto_increment, post_id bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, meta_value longtext, PRIMARY KEY  (meta_id), KEY post_id (post_id) )";
const T: &str = "CREATE TABLE t ( a int NOT NULL, b varchar(10), term_id bigint, PRIMARY KEY (a) )";
const SCHEMA: &[&str] = &[WP_OPTIONS, WP_POSTS, WP_POSTMETA, T];

fn text(s: &str) -> DbValue {
    DbValue::Text(s.into())
}

fn run(sql: &str, params: Vec<DbValue>) -> (Vec<SqliteStep>, Option<String>, bool) {
    match plan_with(sql, params, SCHEMA) {
        SqlitePlan::Run {
            steps,
            found_rows,
            information_schema,
        } => (steps, found_rows.map(|f| f.sql), information_schema),
        other => panic!("{sql}: {other:?}"),
    }
}

/// The one step `sql` plans to, checked against `expected`.
fn one(sql: &str, params: Vec<DbValue>, expected: &str) -> SqliteStep {
    let (mut steps, _, _) = run(sql, params);
    assert_eq!(steps.len(), 1, "{steps:?}");
    let step = steps.remove(0);
    assert_sqlite_sql(&step.sql.sql, expected);
    step
}

fn unsupported(construct: &str) -> DbError {
    DbError::Unsupported {
        construct: construct.into(),
        dialect: DialectKind::Sqlite,
    }
}

#[test]
fn update_counts_changed_rows() {
    let step = one(
        "UPDATE wp_options SET option_value = 'b', autoload = 'auto' WHERE option_name = 'a'",
        vec![],
        r#"UPDATE "wp_options" SET "option_value" = 'b', "autoload" = 'auto' WHERE ("option_name" = 'a') AND ("option_value" IS NOT 'b' COLLATE BINARY OR "autoload" IS NOT 'auto' COLLATE BINARY)"#,
    );
    assert_eq!(
        (step.count, step.insert_id, step.returns_rows),
        (CountRule::Changes, InsertIdRule::None, false)
    );
}

#[test]
fn update_limit_predicate_outside_row_ids() {
    one(
        "UPDATE wp_posts SET post_status = 'draft' WHERE post_author = 1 LIMIT 5",
        vec![],
        r#"UPDATE "wp_posts" SET "post_status" = 'draft' WHERE (rowid IN (SELECT rowid FROM "wp_posts" WHERE "post_author" = 1 LIMIT 5)) AND ("post_status" IS NOT 'draft' COLLATE BINARY)"#,
    );
}

#[test]
fn update_nondeterministic_has_no_predicate() {
    one(
        "UPDATE t SET a = RAND()",
        vec![],
        r#"UPDATE "t" SET "a" = RAND()"#,
    );
}

#[test]
fn update_join() {
    let params = vec![text("k"), text("s"), text("v")];
    let step = one(
        "UPDATE wp_posts p JOIN wp_postmeta m ON m.post_id = p.ID AND m.meta_key = ? SET p.post_status = ? WHERE m.meta_value = ?",
        params.clone(),
        r#"UPDATE "wp_posts" AS "p" SET "post_status" = ?2 FROM "wp_postmeta" AS "m" WHERE ("m"."post_id" = "p"."ID" AND "m"."meta_key" = ?1) AND ("m"."meta_value" = ?3) AND ("p"."post_status" IS NOT ?2 COLLATE BINARY)"#,
    );
    assert_eq!(step.sql.params, params);
    assert_eq!(step.count, CountRule::Changes);
}

#[test]
fn upsert_counts_and_returns_row_id() {
    let step = one(
        "INSERT INTO wp_options (option_name, option_value, autoload) VALUES ('a', 'b', 'auto') ON DUPLICATE KEY UPDATE option_name = VALUES(option_name), option_value = VALUES(option_value), autoload = VALUES(autoload)",
        vec![],
        r#"INSERT INTO "wp_options" ("option_name", "option_value", "autoload") VALUES ('a', 'b', 'auto') ON CONFLICT DO UPDATE SET "option_name" = excluded."option_name", "option_value" = excluded."option_value", "autoload" = excluded."autoload" WHERE "option_name" IS NOT excluded."option_name" COLLATE BINARY OR "option_value" IS NOT excluded."option_value" COLLATE BINARY OR "autoload" IS NOT excluded."autoload" COLLATE BINARY RETURNING rowid"#,
    );
    assert_eq!(
        (step.count, step.insert_id),
        (CountRule::Upsert, InsertIdRule::Returning)
    );
}

#[test]
fn upsert_multi_row_splits() {
    let params: Vec<DbValue> = (1..=5).map(DbValue::Int).collect();
    let (steps, _, _) = run(
        "INSERT INTO t (a, b) VALUES (?, ?), (?, ?) ON DUPLICATE KEY UPDATE b = ?",
        params.clone(),
    );
    assert_eq!(steps.len(), 2, "{steps:?}");
    assert_sqlite_sql(
        &steps[0].sql.sql,
        r#"INSERT INTO "t" ("a", "b") VALUES (?1, ?2) ON CONFLICT DO UPDATE SET "b" = ?5 WHERE "b" IS NOT ?5 COLLATE BINARY"#,
    );
    assert_sqlite_sql(
        &steps[1].sql.sql,
        r#"INSERT INTO "t" ("a", "b") VALUES (?3, ?4) ON CONFLICT DO UPDATE SET "b" = ?5 WHERE "b" IS NOT ?5 COLLATE BINARY"#,
    );
    for step in &steps {
        assert_eq!(step.sql.params, params);
        assert_eq!(
            (step.count, step.insert_id),
            (CountRule::Upsert, InsertIdRule::LastRowid)
        );
    }
}

#[test]
fn replace_deletes_then_inserts() {
    let (steps, _, _) = run(
        "REPLACE INTO wp_options (option_name, option_value, autoload) VALUES ('a', 'b', 'no')",
        vec![],
    );
    assert_eq!(steps.len(), 2, "{steps:?}");
    assert_sqlite_sql(
        &steps[0].sql.sql,
        r#"DELETE FROM "wp_options" WHERE ("option_name" = 'a')"#,
    );
    assert_sqlite_sql(
        &steps[1].sql.sql,
        r#"INSERT INTO "wp_options" ("option_name", "option_value", "autoload") VALUES ('a', 'b', 'no')"#,
    );
    assert_eq!(
        (steps[0].count, steps[0].insert_id),
        (CountRule::Changes, InsertIdRule::None)
    );
    assert_eq!(
        (steps[1].count, steps[1].insert_id),
        (CountRule::Changes, InsertIdRule::LastRowid)
    );
}

#[test]
fn multi_row_insert_returns_row_ids() {
    let step = one(
        "INSERT INTO wp_options (option_name) VALUES ('a'), ('b')",
        vec![],
        r#"INSERT INTO "wp_options" ("option_name") VALUES ('a'), ('b') RETURNING rowid"#,
    );
    assert_eq!(
        (step.count, step.insert_id),
        (CountRule::Changes, InsertIdRule::Returning)
    );
}

#[test]
fn delete_two_targets_through_temp_table() {
    let (steps, _, _) = run(
        r"DELETE a, b FROM wp_options a, wp_options b WHERE a.option_name LIKE '\\_transient\\_%' AND a.option_name NOT LIKE '\\_transient\\_timeout\\_%' AND b.option_name = CONCAT('_transient_timeout_', SUBSTRING(a.option_name, 12)) AND b.option_value < 1700000000",
        vec![],
    );
    let sql: Vec<&str> = steps.iter().map(|s| s.sql.sql.as_str()).collect();
    assert_eq!(sql.len(), 5, "{sql:?}");
    assert_sqlite_sql(sql[0], r#"DROP TABLE IF EXISTS temp."speedyspec_delete""#);
    assert_sqlite_sql(
        sql[1],
        r#"CREATE TEMP TABLE "speedyspec_delete" AS SELECT "a".rowid AS "r0", "b".rowid AS "r1" FROM "wp_options" AS "a", "wp_options" AS "b" WHERE "a"."option_name" LIKE '\_transient\_%' ESCAPE '\' AND "a"."option_name" NOT LIKE '\_transient\_timeout\_%' ESCAPE '\' AND "b"."option_name" = CONCAT('_transient_timeout_', SUBSTRING("a"."option_name", 12)) AND CAST("b"."option_value" AS REAL) < 1700000000"#,
    );
    assert_sqlite_sql(
        sql[2],
        r#"DELETE FROM "wp_options" WHERE rowid IN (SELECT "r0" FROM temp."speedyspec_delete")"#,
    );
    assert_sqlite_sql(
        sql[3],
        r#"DELETE FROM "wp_options" WHERE rowid IN (SELECT "r1" FROM temp."speedyspec_delete")"#,
    );
    assert_sqlite_sql(sql[4], r#"DROP TABLE temp."speedyspec_delete""#);
    let counts: Vec<CountRule> = steps.iter().map(|s| s.count).collect();
    assert_eq!(
        counts,
        [
            CountRule::None,
            CountRule::None,
            CountRule::Changes,
            CountRule::Changes,
            CountRule::None
        ]
    );
}

#[test]
fn delete_one_target_of_a_join() {
    one(
        "DELETE m FROM wp_postmeta m JOIN wp_posts p ON p.ID = m.post_id WHERE p.post_type = 'x'",
        vec![],
        r#"DELETE FROM "wp_postmeta" WHERE rowid IN (SELECT "m".rowid FROM "wp_postmeta" AS "m" JOIN "wp_posts" AS "p" ON "p"."ID" = "m"."post_id" WHERE "p"."post_type" = 'x')"#,
    );
}

#[test]
fn group_concat() {
    one(
        "SELECT GROUP_CONCAT(term_id ORDER BY term_id SEPARATOR ';') FROM t",
        vec![],
        r#"SELECT group_concat("term_id", ';' ORDER BY "term_id") FROM "t""#,
    );
}

#[test]
fn integer_and_real_division() {
    one(
        "SELECT a DIV b, a / b FROM t",
        vec![],
        r#"SELECT CAST("a" AS INTEGER) / CAST("b" AS INTEGER), CAST("a" AS REAL) / "b" FROM "t""#,
    );
}

#[test]
fn substring_from_for() {
    one(
        "SELECT SUBSTRING(post_title FROM 2 FOR 3) FROM wp_posts",
        vec![],
        r#"SELECT SUBSTRING("post_title", 2, 3) FROM "wp_posts""#,
    );
}

#[test]
fn last_insert_id() {
    one(
        "SELECT LAST_INSERT_ID()",
        vec![],
        r#"SELECT last_insert_rowid() AS "LAST_INSERT_ID()""#,
    );
}

#[test]
fn cast_date_and_rlike() {
    let step = one(
        "SELECT CAST(post_date AS DATE) FROM wp_posts WHERE post_name RLIKE '^a'",
        vec![],
        r#"SELECT date("post_date") FROM "wp_posts" WHERE "post_name" REGEXP '^a'"#,
    );
    assert!(step.returns_rows);
}

#[test]
fn casts() {
    one(
        "SELECT CAST(a AS DATETIME), CAST(a AS TIME), CAST(a AS DECIMAL(10,2)), CAST(a AS BINARY) FROM t",
        vec![],
        r#"SELECT CAST("a" AS TEXT), CAST("a" AS TEXT), CAST("a" AS REAL), "a" COLLATE BINARY FROM "t""#,
    );
}

#[test]
fn binary_comparisons() {
    one(
        "SELECT * FROM wp_posts WHERE BINARY post_name = 'Hello' AND post_title LIKE BINARY 'He%'",
        vec![],
        r#"SELECT * FROM "wp_posts" WHERE "post_name" COLLATE BINARY = 'Hello' AND "post_title" LIKE CAST('He%' AS BLOB) ESCAPE '\'"#,
    );
}

#[test]
fn mysql_collations() {
    one(
        "SELECT * FROM t WHERE b = 'x' COLLATE utf8mb4_bin OR b = 'y' COLLATE utf8mb4_unicode_520_ci",
        vec![],
        r#"SELECT * FROM "t" WHERE "b" = 'x' COLLATE BINARY OR "b" = 'y' COLLATE speedyspec_ci"#,
    );
}

#[test]
fn for_update_dropped() {
    one(
        "SELECT * FROM wp_options WHERE option_name = 'a' FOR UPDATE",
        vec![],
        r#"SELECT * FROM "wp_options" WHERE "option_name" = 'a'"#,
    );
}

#[test]
fn information_schema_tables() {
    let (steps, _, information_schema) = run(
        "SELECT table_name, data_length FROM information_schema.tables WHERE table_schema = DATABASE()",
        vec![],
    );
    assert_sqlite_sql(
        &steps[0].sql.sql,
        r#"SELECT "table_name" AS "TABLE_NAME", "data_length" AS "DATA_LENGTH" FROM "information_schema"."tables" WHERE "table_schema" = DATABASE()"#,
    );
    assert!(information_schema);
}

#[test]
fn text_column_against_number_compares_numerically() {
    let step = one(
        "SELECT meta_id FROM wp_postmeta WHERE meta_value > ?",
        vec![DbValue::Int(5)],
        r#"SELECT "meta_id" FROM "wp_postmeta" WHERE CAST("meta_value" AS REAL) > ?1"#,
    );
    assert_eq!(step.sql.params, [DbValue::Int(5)]);
    one(
        "SELECT meta_id FROM wp_postmeta WHERE meta_value > ?",
        vec![text("5")],
        r#"SELECT "meta_id" FROM "wp_postmeta" WHERE "meta_value" > ?1"#,
    );
}

#[test]
fn found_rows() {
    let (steps, found_rows, _) = run(
        "SELECT SQL_CALC_FOUND_ROWS wp_posts.ID FROM wp_posts WHERE 1=1 AND post_type = ? ORDER BY wp_posts.post_date DESC LIMIT 0, 10",
        vec![text("post")],
    );
    assert_sqlite_sql(
        &steps[0].sql.sql,
        r#"SELECT "wp_posts"."ID" FROM "wp_posts" WHERE 1 = 1 AND "post_type" = ?1 ORDER BY "wp_posts"."post_date" DESC LIMIT 10 OFFSET 0"#,
    );
    assert_sqlite_sql(
        &found_rows.expect("a count"),
        r#"SELECT COUNT(*) FROM (SELECT "wp_posts"."ID" FROM "wp_posts" WHERE 1 = 1 AND "post_type" = ?1) AS "speedyspec_found_rows""#,
    );
}

#[test]
fn transactions_take_the_write_lock() {
    one("START TRANSACTION", vec![], "BEGIN IMMEDIATE");
    one("BEGIN", vec![], "BEGIN IMMEDIATE");
    one("START TRANSACTION READ ONLY", vec![], "BEGIN DEFERRED");
}

#[test]
fn refusals() {
    for (sql, construct) in [
        ("REPLACE INTO t SELECT * FROM u", "REPLACE ... SELECT"),
        (
            "UPDATE t a JOIN t b ON a.a = b.a SET a.b = 1, b.b = 2",
            "UPDATE that sets columns of two tables",
        ),
        (
            "UPDATE t a LEFT JOIN t b ON a.a = b.a SET a.b = 1",
            "UPDATE … LEFT JOIN",
        ),
        (
            "SELECT GROUP_CONCAT(DISTINCT a SEPARATOR ';') FROM t",
            "GROUP_CONCAT(DISTINCT ... SEPARATOR)",
        ),
        (
            "SELECT ID FROM wp_posts WHERE MATCH (post_title) AGAINST ('x')",
            "MATCH ... AGAINST",
        ),
        (
            "SELECT * FROM information_schema.processlist",
            "information_schema.processlist",
        ),
    ] {
        assert_eq!(plan_error(sql, SCHEMA), unsupported(construct), "{sql}");
    }
}

/// `sqlparser` 0.64 reads `LOCK IN SHARE MODE` as an error: a parser gap.
#[test]
fn lock_in_share_mode_parser_gap() {
    assert!(matches!(
        speedyspec_data_query::parse_one("SELECT * FROM t LOCK IN SHARE MODE"),
        Err(DbError::Parse { .. })
    ));
}

/// The translated statements run on SQLite and count rows as MySQL does.
#[test]
fn dml_steps_run_on_sqlite() {
    use rusqlite::types::Value as Sql;
    use speedyspec_data_sqlite::{SqliteTarget, open_connection};
    use speedyspec_data_translate::SessionVars;
    use std::sync::{Arc, Mutex};

    let conn = open_connection(
        &SqliteTarget::Memory,
        std::time::Duration::from_secs(1),
        Arc::new(Mutex::new(SessionVars::mysql_defaults("main", "8.4.0"))),
    )
    .unwrap();
    // Runs every step of `sql`, binding only the params each step names; the changes of the counted steps.
    let run = |sql: &str, params: Vec<DbValue>| -> usize {
        let (steps, _, _) = run(sql, params);
        let mut changes = 0;
        for step in steps {
            let mut stmt = conn
                .prepare(&step.sql.sql)
                .unwrap_or_else(|e| panic!("{}: {e}", step.sql.sql));
            for i in 1..=stmt.parameter_count() {
                let v = match &step.sql.params[i - 1] {
                    DbValue::Int(n) => Sql::Integer(*n),
                    DbValue::Text(s) => Sql::Text(s.clone()),
                    other => panic!("{other:?}"),
                };
                stmt.raw_bind_parameter(i, v).unwrap();
            }
            if stmt.column_count() > 0 {
                let mut rows = stmt.raw_query();
                while rows.next().unwrap().is_some() {}
            } else {
                stmt.raw_execute()
                    .unwrap_or_else(|e| panic!("{}: {e}", step.sql.sql));
            }
            if step.count != CountRule::None {
                changes += conn.changes() as usize;
            }
        }
        changes
    };
    let catalogue: Vec<String> = speedyspec_data_translate::catalogue_ddl()
        .iter()
        .map(|s| s.tree().to_string())
        .collect();
    for ddl in catalogue
        .iter()
        .map(String::as_str)
        .chain(SCHEMA.iter().copied())
    {
        for step in support::plan_steps(ddl, &[]) {
            conn.execute_batch(&step)
                .unwrap_or_else(|e| panic!("{step}: {e}"));
        }
    }
    assert_eq!(
        run(
            "INSERT INTO wp_options (option_name, option_value) VALUES ('a', 'x'), ('b', 'y')",
            vec![]
        ),
        2
    );
    // Same value: matched, not changed.
    assert_eq!(
        run(
            "UPDATE wp_options SET option_value = 'x' WHERE option_name = 'a'",
            vec![]
        ),
        0
    );
    // A change in case is a change.
    assert_eq!(
        run(
            "UPDATE wp_options SET option_value = 'X' WHERE option_name = 'A'",
            vec![]
        ),
        1
    );
    assert_eq!(
        run(
            "INSERT INTO wp_options (option_name, option_value) VALUES ('a', 'X') ON DUPLICATE KEY UPDATE option_value = VALUES(option_value)",
            vec![]
        ),
        0
    );
    assert_eq!(
        run(
            "REPLACE INTO wp_options (option_name, option_value, autoload) VALUES ('a', 'z', 'no')",
            vec![]
        ),
        2
    );
    let params = vec![
        text("_transient_k"),
        text("_transient_timeout_k"),
        text("100"),
    ];
    run(
        "INSERT INTO wp_options (option_name, option_value) VALUES (?, 'v'), (?, ?)",
        params,
    );
    assert_eq!(
        run(
            r"DELETE a, b FROM wp_options a, wp_options b WHERE a.option_name LIKE '\\_transient\\_%' AND a.option_name NOT LIKE '\\_transient\\_timeout\\_%' AND b.option_name = CONCAT('_transient_timeout_', SUBSTRING(a.option_name, 12)) AND b.option_value < 1700000000",
            vec![],
        ),
        2
    );
    run(
        "INSERT INTO wp_posts (post_title, post_status) VALUES ('t', 'publish')",
        vec![],
    );
    run(
        "INSERT INTO wp_postmeta (post_id, meta_key, meta_value) VALUES (1, 'k', 'v')",
        vec![],
    );
    let params = vec![text("k"), text("draft"), text("v")];
    let sql = "UPDATE wp_posts p JOIN wp_postmeta m ON m.post_id = p.ID AND m.meta_key = ? SET p.post_status = ? WHERE m.meta_value = ?";
    assert_eq!(run(sql, params.clone()), 1);
    assert_eq!(run(sql, params), 0);
    assert_eq!(
        run(
            "DELETE m FROM wp_postmeta m JOIN wp_posts p ON p.ID = m.post_id WHERE p.post_status = 'draft'",
            vec![]
        ),
        1
    );
    let left: i64 = conn
        .query_row("SELECT count(*) FROM wp_options", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 2);
}
