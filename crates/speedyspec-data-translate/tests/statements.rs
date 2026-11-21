mod support;

use speedyspec_data::{DbError, EmulatedOp};
use speedyspec_data_query::{Rewritten, parse_one};
use speedyspec_data_translate::{AdminPass, InsertPass, RowLimitPass, take_found_rows};
use support::*;

fn check(pass: impl speedyspec_data_query::RewritePass + 'static, sql: &str, out: &str) {
    assert_sql(&statement_of(rewrite_one(pass, sql, &cx()).unwrap()), out);
}

#[test]
fn insert_on_duplicate_key_update() {
    check(
        InsertPass,
        "INSERT INTO wp_options (option_name, option_value, autoload) VALUES ('a', 'b', 'auto') ON DUPLICATE KEY UPDATE option_name = VALUES(option_name), option_value = VALUES(option_value), autoload = VALUES(autoload)",
        "INSERT INTO wp_options (option_name, option_value, autoload) VALUES ('a', 'b', 'auto') ON CONFLICT DO UPDATE SET option_name = excluded.option_name, option_value = excluded.option_value, autoload = excluded.autoload",
    );
}

#[test]
fn insert_row_alias() {
    check(
        InsertPass,
        "INSERT INTO t (a, b) VALUES (1, 2) AS new ON DUPLICATE KEY UPDATE b = new.b + t.b",
        "INSERT INTO t (a, b) VALUES (1, 2) ON CONFLICT DO UPDATE SET b = excluded.b + t.b",
    );
}

#[test]
fn insert_ignore() {
    check(
        InsertPass,
        "INSERT IGNORE INTO wp_term_relationships (object_id, term_taxonomy_id) VALUES (1, 2)",
        "INSERT INTO wp_term_relationships (object_id, term_taxonomy_id) VALUES (1, 2) ON CONFLICT DO NOTHING",
    );
}

#[test]
fn insert_set_form() {
    check(
        InsertPass,
        "INSERT INTO wp_postmeta SET post_id = 1, meta_key = 'k', meta_value = 'v'",
        "INSERT INTO wp_postmeta (post_id, meta_key, meta_value) VALUES (1, 'k', 'v')",
    );
}

#[test]
fn row_limit_delete() {
    check(
        RowLimitPass::new("rowid"),
        "DELETE FROM wp_options WHERE option_name LIKE 'x%' ORDER BY option_id LIMIT 100",
        "DELETE FROM wp_options WHERE rowid IN (SELECT rowid FROM wp_options WHERE option_name LIKE 'x%' ORDER BY option_id LIMIT 100)",
    );
}

#[test]
fn row_limit_update() {
    check(
        RowLimitPass::new("rowid"),
        "UPDATE wp_posts SET post_status = 'draft' WHERE post_author = 1 LIMIT 5",
        "UPDATE wp_posts SET post_status = 'draft' WHERE rowid IN (SELECT rowid FROM wp_posts WHERE post_author = 1 LIMIT 5)",
    );
    // Without ORDER BY or LIMIT, nothing changes.
    check(
        RowLimitPass::new("ctid"),
        "DELETE FROM t WHERE a = 1",
        "DELETE FROM t WHERE a = 1",
    );
}

#[test]
fn found_rows_counts_without_order_and_limit() {
    let mut stmt = parse_one(
        "SELECT SQL_CALC_FOUND_ROWS wp_posts.ID FROM wp_posts WHERE 1=1 AND post_type = ?1 ORDER BY wp_posts.post_date DESC LIMIT ?2, 10",
    )
    .unwrap();
    let count = take_found_rows(&mut stmt).expect("a count");
    assert_eq!(
        stmt.tree().to_string(),
        "SELECT wp_posts.ID FROM wp_posts WHERE 1 = 1 AND post_type = ?1 ORDER BY wp_posts.post_date DESC LIMIT ?2, 10"
    );
    assert_sql(
        &count,
        "SELECT COUNT(*) FROM (SELECT wp_posts.ID FROM wp_posts WHERE 1=1 AND post_type = ?1) AS speedyspec_found_rows",
    );
}

#[test]
fn found_rows_absent_is_none() {
    let mut stmt = parse_one("SELECT ID FROM wp_posts LIMIT 10").unwrap();
    assert!(take_found_rows(&mut stmt).is_none());
}

fn emulated(sql: &str) -> EmulatedOp {
    match rewrite_one(AdminPass, sql, &cx()).unwrap() {
        Rewritten::Emulated(op) => op,
        other => panic!("{sql}: {other:?}"),
    }
}

#[test]
fn admin_emulations() {
    let like = |p: &str| Some(p.to_string());
    for (sql, op) in [
        ("SHOW TABLES", EmulatedOp::ShowTables { like: None }),
        (
            r"SHOW TABLES LIKE 'wp\\_%'",
            EmulatedOp::ShowTables {
                like: like(r"wp\_%"),
            },
        ),
        (
            "SHOW FULL COLUMNS FROM `wp_posts`",
            EmulatedOp::ShowColumns {
                table: "wp_posts".into(),
                full: true,
                like: None,
            },
        ),
        (
            "SHOW COLUMNS FROM t LIKE 'a'",
            EmulatedOp::ShowColumns {
                table: "t".into(),
                full: false,
                like: like("a"),
            },
        ),
        (
            "SHOW FIELDS FROM t",
            EmulatedOp::ShowColumns {
                table: "t".into(),
                full: false,
                like: None,
            },
        ),
        (
            "DESCRIBE wp_posts",
            EmulatedOp::ShowColumns {
                table: "wp_posts".into(),
                full: false,
                like: None,
            },
        ),
        (
            "DESC wp_posts",
            EmulatedOp::ShowColumns {
                table: "wp_posts".into(),
                full: false,
                like: None,
            },
        ),
        (
            "EXPLAIN wp_posts",
            EmulatedOp::ShowColumns {
                table: "wp_posts".into(),
                full: false,
                like: None,
            },
        ),
        (
            "SHOW INDEX FROM wp_posts",
            EmulatedOp::ShowIndex {
                table: "wp_posts".into(),
            },
        ),
        (
            "SHOW INDEXES FROM t",
            EmulatedOp::ShowIndex { table: "t".into() },
        ),
        (
            "SHOW KEYS FROM t",
            EmulatedOp::ShowIndex { table: "t".into() },
        ),
        (
            "SHOW CREATE TABLE t",
            EmulatedOp::ShowCreateTable { table: "t".into() },
        ),
        (
            "SHOW VARIABLES LIKE 'max%'",
            EmulatedOp::ShowVariables { like: like("max%") },
        ),
        (
            "SHOW SESSION VARIABLES",
            EmulatedOp::ShowVariables { like: None },
        ),
        (
            "SHOW TABLE STATUS",
            EmulatedOp::ShowTableStatus { like: None },
        ),
        (
            "SET NAMES 'utf8mb4' COLLATE 'utf8mb4_unicode_520_ci'",
            EmulatedOp::SetSession {
                name: "names".into(),
                value: "utf8mb4 utf8mb4_unicode_520_ci".into(),
            },
        ),
        (
            "SET NAMES utf8mb4",
            EmulatedOp::SetSession {
                name: "names".into(),
                value: "utf8mb4".into(),
            },
        ),
        (
            "SET @@SESSION.sql_mode = 'NO_ENGINE_SUBSTITUTION'",
            EmulatedOp::SetSession {
                name: "sql_mode".into(),
                value: "NO_ENGINE_SUBSTITUTION".into(),
            },
        ),
        (
            "SET SESSION SQL_MODE = 'x'",
            EmulatedOp::SetSession {
                name: "sql_mode".into(),
                value: "x".into(),
            },
        ),
        (
            "SET @@wait_timeout = 30",
            EmulatedOp::SetSession {
                name: "wait_timeout".into(),
                value: "30".into(),
            },
        ),
        ("SELECT FOUND_ROWS()", EmulatedOp::FoundRows),
        ("SELECT FOUND_ROWS() AS n", EmulatedOp::FoundRows),
        ("LOCK TABLES t WRITE", EmulatedOp::Noop),
        ("UNLOCK TABLES", EmulatedOp::Noop),
        ("ANALYZE TABLE t", EmulatedOp::Noop),
        (
            "SET autocommit = 1",
            EmulatedOp::SetSession {
                name: "autocommit".into(),
                value: "1".into(),
            },
        ),
        (
            "SET autocommit = 0",
            EmulatedOp::SetSession {
                name: "autocommit".into(),
                value: "0".into(),
            },
        ),
        (
            "SHOW VARIABLES WHERE Variable_name='collation_connection'",
            EmulatedOp::ShowVariables {
                like: like("collation\\_connection"),
            },
        ),
        (
            "USE `wordpress`",
            EmulatedOp::SetSession {
                name: "database".into(),
                value: "wordpress".into(),
            },
        ),
        (
            "SHOW VARIABLES WHERE Variable_name LIKE 'max%'",
            EmulatedOp::ShowVariables { like: like("max%") },
        ),
    ] {
        assert_eq!(emulated(sql), op, "{sql}");
    }
}

#[test]
fn admin_refusals() {
    for (sql, construct) in [
        ("SET @a = 1", "user variable"),
        ("SHOW VARIABLES WHERE Value = 'x'", "SHOW VARIABLES WHERE"),
        ("SELECT FOUND_ROWS() + 1", "FOUND_ROWS() in an expression"),
        (
            "SHOW TABLE STATUS LIKE 'wp_%'",
            "SHOW TABLE STATUS LIKE (the parser drops the pattern)",
        ),
    ] {
        match rewrite_one(AdminPass, sql, &cx()) {
            Err(DbError::Unsupported {
                construct: c,
                dialect,
            }) => {
                assert_eq!((c.as_str(), dialect), (construct, cx().target), "{sql}");
            }
            other => panic!("{sql}: {other:?}"),
        }
    }
}

#[test]
fn admin_parser_gaps() {
    // sqlparser 0.64 does not parse these MySQL maintenance statements: parser gaps, listed in the PR.
    for sql in ["OPTIMIZE TABLE t", "CHECK TABLE t", "REPAIR TABLE t"] {
        assert!(
            matches!(parse_one(sql), Err(DbError::Parse { .. })),
            "{sql}"
        );
    }
}

#[test]
fn ordinary_statements_pass_through_admin() {
    check(AdminPass, "SELECT a FROM t", "SELECT a FROM t");
}
