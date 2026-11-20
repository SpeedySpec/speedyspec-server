//! MySQL DDL as SQLite runs it: types, AUTO_INCREMENT, implicit defaults, ALTER and table rebuilds.

mod support;

use speedyspec_data::{DbError, Dialect, DialectKind};
use speedyspec_data_query::parse_one;
use speedyspec_data_sqlite::{CountRule, SqliteDialect};
use support::{assert_sqlite_sql, is_catalogue_write, plan_run, plan_steps};

const WP_OPTIONS: &str = "CREATE TABLE wp_options ( option_id bigint(20) unsigned NOT NULL auto_increment, option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY  (option_id), UNIQUE KEY option_name (option_name), KEY autoload (autoload) ) DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_520_ci";

fn assert_counts_none(sql: &str, ddl: &[&str]) {
    for step in plan_run(sql, ddl) {
        assert_eq!(step.count, CountRule::None, "{}", step.sql.sql);
    }
}

#[test]
fn create_wp_options() {
    let steps = plan_steps(WP_OPTIONS, &[]);
    assert_sqlite_sql(
        &steps[0],
        r#"CREATE TABLE "wp_options" ("option_id" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, "option_name" TEXT NOT NULL DEFAULT '' COLLATE speedyspec_ci, "option_value" TEXT NOT NULL DEFAULT '' COLLATE speedyspec_ci, "autoload" TEXT NOT NULL DEFAULT 'yes' COLLATE speedyspec_ci)"#,
    );
    assert_sqlite_sql(
        &steps[1],
        r#"CREATE UNIQUE INDEX "wp_options__option_name" ON "wp_options" ("option_name")"#,
    );
    assert_sqlite_sql(
        &steps[2],
        r#"CREATE INDEX "wp_options__autoload" ON "wp_options" ("autoload")"#,
    );
    assert!(
        steps[3..].iter().all(|s| is_catalogue_write(s)),
        "{steps:?}"
    );
    assert_counts_none(WP_OPTIONS, &[]);
}

#[test]
fn create_type_map_and_on_update() {
    let sql = "CREATE TABLE t (id int NOT NULL, a tinyint(1) NOT NULL DEFAULT 0, b decimal(10,2), c double, d datetime NOT NULL DEFAULT '0000-00-00 00:00:00', e varbinary(16), f enum('x','y') NOT NULL, g varchar(10) COLLATE utf8mb4_bin, h timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP, PRIMARY KEY (id))";
    let steps = plan_steps(sql, &[]);
    assert_sqlite_sql(
        &steps[0],
        r#"CREATE TABLE "t" ("id" INTEGER NOT NULL, "a" INTEGER NOT NULL DEFAULT 0, "b" REAL, "c" REAL, "d" TEXT NOT NULL DEFAULT '0000-00-00 00:00:00', "e" BLOB, "f" TEXT NOT NULL DEFAULT 'x' COLLATE speedyspec_ci, "g" TEXT COLLATE BINARY, "h" TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, PRIMARY KEY ("id"))"#,
    );
    assert_eq!(
        steps[1],
        r#"CREATE TRIGGER "t__h__on_update" AFTER UPDATE ON "t" FOR EACH ROW WHEN NEW."h" IS OLD."h" BEGIN UPDATE "t" SET "h" = CURRENT_TIMESTAMP WHERE rowid = NEW.rowid; END"#
    );
    assert!(
        steps[2..].iter().all(|s| is_catalogue_write(s)),
        "{steps:?}"
    );
    assert_counts_none(sql, &[]);
}

#[test]
fn add_column_implicit_default() {
    let ddl = ["CREATE TABLE t (id int NOT NULL, b int, PRIMARY KEY (id))"];
    let sql = "ALTER TABLE t ADD COLUMN c varchar(10) NOT NULL";
    let steps = plan_steps(sql, &ddl);
    assert_sqlite_sql(
        &steps[0],
        r#"ALTER TABLE "t" ADD COLUMN "c" TEXT NOT NULL DEFAULT '' COLLATE speedyspec_ci"#,
    );
    assert!(
        steps[1..].iter().all(|s| is_catalogue_write(s)),
        "{steps:?}"
    );
    assert_counts_none(sql, &ddl);
}

#[test]
fn change_column_catalogue_only() {
    let ddl =
        ["CREATE TABLE t (id int NOT NULL, c varchar(10) NOT NULL DEFAULT '', PRIMARY KEY (id))"];
    let sql = "ALTER TABLE t CHANGE COLUMN c c varchar(255) NOT NULL DEFAULT ''";
    let steps = plan_steps(sql, &ddl);
    assert!(!steps.is_empty());
    assert!(steps.iter().all(|s| is_catalogue_write(s)), "{steps:?}");
    assert_counts_none(sql, &ddl);
}

#[test]
fn modify_column_rebuilds() {
    let ddl = ["CREATE TABLE t (id int NOT NULL, c varchar(10), PRIMARY KEY (id), KEY c (c))"];
    let sql = "ALTER TABLE t MODIFY c bigint(20) NOT NULL DEFAULT 0";
    let steps = plan_steps(sql, &ddl);
    assert_sqlite_sql(
        &steps[0],
        r#"CREATE TABLE "t__speedyspec_rebuild" ("id" INTEGER NOT NULL, "c" INTEGER NOT NULL DEFAULT 0, PRIMARY KEY ("id"))"#,
    );
    assert_sqlite_sql(
        &steps[1],
        r#"INSERT INTO "t__speedyspec_rebuild" ("id", "c") SELECT "id", "c" FROM "t""#,
    );
    assert_sqlite_sql(&steps[2], r#"DROP TABLE "t""#);
    assert_sqlite_sql(
        &steps[3],
        r#"ALTER TABLE "t__speedyspec_rebuild" RENAME TO "t""#,
    );
    assert_sqlite_sql(&steps[4], r#"CREATE INDEX "t__c" ON "t" ("c")"#);
    assert!(steps.len() > 5);
    assert!(
        steps[5..].iter().all(|s| is_catalogue_write(s)),
        "{steps:?}"
    );
    assert_counts_none(sql, &ddl);
}

#[test]
fn rename_table_renames_indexes() {
    let ddl = ["CREATE TABLE a (id int NOT NULL, x int, PRIMARY KEY (id), KEY k (x))"];
    let sql = "ALTER TABLE a RENAME TO b";
    let steps = plan_steps(sql, &ddl);
    assert_sqlite_sql(&steps[0], r#"ALTER TABLE "a" RENAME TO "b""#);
    assert_sqlite_sql(&steps[1], r#"DROP INDEX IF EXISTS "a__k""#);
    assert_sqlite_sql(&steps[2], r#"CREATE INDEX "b__k" ON "b" ("x")"#);
    assert!(steps.len() > 3);
    assert!(
        steps[3..].iter().all(|s| is_catalogue_write(s)),
        "{steps:?}"
    );
    assert_counts_none(sql, &ddl);
}

#[test]
fn truncate_resets_sequence() {
    let sql = "TRUNCATE TABLE wp_t";
    let steps = plan_steps(sql, &[]);
    assert_eq!(steps.len(), 2, "{steps:?}");
    assert_sqlite_sql(&steps[0], r#"DELETE FROM "wp_t""#);
    assert_sqlite_sql(
        &steps[1],
        r#"DELETE FROM sqlite_sequence WHERE "name" = 'wp_t'"#,
    );
    assert_counts_none(sql, &[]);
}

#[test]
fn autoincrement_outside_pk_unsupported() {
    let stmt =
        parse_one("CREATE TABLE t (a int AUTO_INCREMENT, b int, PRIMARY KEY (a, b))").unwrap();
    let err = SqliteDialect::new().plan(&stmt, None).unwrap_err();
    assert_eq!(
        err,
        DbError::Unsupported {
            construct: "AUTO_INCREMENT outside a single-column primary key".into(),
            dialect: DialectKind::Sqlite,
        }
    );
}

#[test]
fn prepare_without_introspector() {
    let stmt = parse_one("ALTER TABLE t MODIFY c int").unwrap();
    let err = SqliteDialect::new().prepare(&stmt).unwrap_err();
    assert_eq!(
        err,
        DbError::Unsupported {
            construct: "ALTER TABLE without schema information".into(),
            dialect: DialectKind::Sqlite,
        }
    );
}

/// Every step SQLite is given for these statements runs on a real connection.
#[test]
fn ddl_steps_run_on_sqlite() {
    use speedyspec_data_sqlite::{SqliteTarget, open_connection};
    use speedyspec_data_translate::{SessionVars, catalogue_ddl};
    use std::sync::{Arc, Mutex};

    let conn = open_connection(
        &SqliteTarget::Memory,
        std::time::Duration::from_secs(1),
        Arc::new(Mutex::new(SessionVars::mysql_defaults("main", "8.4.0"))),
    )
    .unwrap();
    let run = |sql: &str, ddl: &[&str]| {
        for step in plan_steps(sql, ddl) {
            conn.execute_batch(&step)
                .unwrap_or_else(|e| panic!("{step}: {e}"));
        }
    };
    for stmt in catalogue_ddl() {
        run(&stmt.tree().to_string(), &[]);
    }
    let t = "CREATE TABLE t (id bigint unsigned NOT NULL AUTO_INCREMENT, c varchar(10), h timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP, PRIMARY KEY (id), KEY c (c))";
    run(WP_OPTIONS, &[]);
    run(t, &[]);
    run("ALTER TABLE t MODIFY c bigint(20) NOT NULL DEFAULT 0", &[t]);
    run("ALTER TABLE t ADD COLUMN d varchar(10) NOT NULL", &[t]);
    run("TRUNCATE TABLE t", &[t]);
    run("ALTER TABLE t RENAME TO u", &[t]);
    let triggers: i64 = conn
        .query_row("SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND name = 't__h__on_update'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(triggers, 1);
}

#[test]
fn rename_keeps_fulltext_in_the_catalogue_only() {
    let ddl = [
        "CREATE TABLE a (id int NOT NULL, x text, PRIMARY KEY (id), KEY k (x(10)), FULLTEXT KEY ft (x))",
    ];
    let steps = plan_steps("ALTER TABLE a RENAME TO b", &ddl);
    assert!(
        !steps
            .iter()
            .any(|s| s.contains("a__ft") || s.contains("b__ft")),
        "{steps:?}"
    );
    assert!(steps.iter().any(|s| s.contains("\"b__k\"")), "{steps:?}");
}

#[test]
fn change_of_a_missing_column_is_mysqls_error() {
    let introspector = speedyspec_data_translate::testing::StaticIntrospector::from_mysql_ddl(&[
        "CREATE TABLE t (a int)",
    ]);
    let stmt = parse_one("ALTER TABLE t CHANGE COLUMN b b int").unwrap();
    let err = SqliteDialect::new()
        .plan(&stmt, Some(&introspector))
        .unwrap_err();
    assert!(
        matches!(&err, DbError::Engine { code: Some(c), message, .. } if c == "1054" && message == "Unknown column 'b' in 't'"),
        "{err:?}"
    );
}
