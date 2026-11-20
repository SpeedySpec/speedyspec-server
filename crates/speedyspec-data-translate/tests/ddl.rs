mod support;

use speedyspec_data::{DbError, DialectKind, EmulatedOp, Statement};
use speedyspec_data_query::{PassContext, Rewritten, parse_one};
use speedyspec_data_translate::testing::StaticIntrospector;
use speedyspec_data_translate::{
    CatalogueIndex, DdlPass, IndexKind, MySqlType, catalogue_table, column_key,
};
use support::*;

const WP_OPTIONS: &str = "CREATE TABLE wp_options ( option_id bigint(20) unsigned NOT NULL auto_increment, option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY  (option_id), UNIQUE KEY option_name (option_name), KEY autoload (autoload) ) DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_520_ci";

fn split(sql: &str, cx: &PassContext) -> Vec<Statement> {
    match rewrite_one(DdlPass, sql, cx).unwrap() {
        Rewritten::Statements(stmts) => stmts,
        other => panic!("{sql}: {other:?}"),
    }
}

/// The statements before the first catalogue write.
fn schema_part(stmts: &[Statement]) -> Vec<String> {
    stmts
        .iter()
        .map(|s| s.tree().to_string())
        .take_while(|s| !s.contains("_speedyspec_"))
        .collect()
}

fn catalogue_part(stmts: &[Statement]) -> String {
    stmts
        .iter()
        .map(|s| s.tree().to_string())
        .filter(|s| s.contains("_speedyspec_"))
        .collect::<Vec<_>>()
        .join(";\n")
}

fn same(actual: &str, expected: &str) {
    let parsed =
        sqlparser::parser::Parser::parse_sql(&sqlparser::dialect::MySqlDialect {}, expected)
            .unwrap();
    assert_eq!(actual, parsed[0].to_string());
}

#[test]
fn ddl_wp_options() {
    let stmts = split(WP_OPTIONS, &cx());
    let schema = schema_part(&stmts);
    assert_eq!(schema.len(), 3, "{schema:?}");
    same(
        &schema[0],
        "CREATE TABLE wp_options (option_id bigint(20) UNSIGNED NOT NULL AUTO_INCREMENT, option_name varchar(191) NOT NULL DEFAULT '', option_value longtext NOT NULL, autoload varchar(20) NOT NULL DEFAULT 'yes', PRIMARY KEY (option_id))",
    );
    same(
        &schema[1],
        "CREATE UNIQUE INDEX wp_options__option_name ON wp_options (option_name)",
    );
    same(
        &schema[2],
        "CREATE INDEX wp_options__autoload ON wp_options (autoload)",
    );
    assert!(stmts.len() > 3);
}

#[test]
fn ddl_prefix_index() {
    let stmts = split(
        "CREATE TABLE wp_postmeta ( meta_id bigint(20) unsigned NOT NULL auto_increment, post_id bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, meta_value longtext, PRIMARY KEY  (meta_id), KEY post_id (post_id), KEY meta_key (meta_key(191)) )",
        &cx(),
    );
    let schema = schema_part(&stmts);
    same(
        &schema[2],
        "CREATE INDEX wp_postmeta__meta_key ON wp_postmeta (meta_key)",
    );
    assert!(
        catalogue_part(&stmts).contains("'meta_key', 191"),
        "{}",
        catalogue_part(&stmts)
    );
}

#[test]
fn ddl_alter_split() {
    let stmts = split(
        "ALTER TABLE wp_t ADD COLUMN c varchar(10) NOT NULL DEFAULT '' AFTER b, ADD KEY c (c), DROP INDEX old",
        &cx(),
    );
    let schema = schema_part(&stmts);
    assert_eq!(schema.len(), 3, "{schema:?}");
    same(
        &schema[0],
        "ALTER TABLE wp_t ADD COLUMN c varchar(10) NOT NULL DEFAULT ''",
    );
    same(&schema[1], "CREATE INDEX wp_t__c ON wp_t (c)");
    same(&schema[2], "DROP INDEX wp_t__old");
    assert!(!catalogue_part(&stmts).is_empty());
}

#[test]
fn ddl_create_index() {
    same(
        &schema_part(&split("CREATE INDEX k ON t (c(10))", &cx()))[0],
        "CREATE INDEX t__k ON t (c)",
    );
    same(
        &schema_part(&split("CREATE UNIQUE INDEX k ON t (c)", &cx()))[0],
        "CREATE UNIQUE INDEX t__k ON t (c)",
    );
}

#[test]
fn ddl_drop_index() {
    same(
        &schema_part(&split("DROP INDEX k ON t", &cx()))[0],
        "DROP INDEX t__k",
    );
}

#[test]
fn ddl_drop_tables() {
    let schema = schema_part(&split("DROP TABLE IF EXISTS a, b", &cx()));
    assert_eq!(
        schema,
        vec![
            "DROP TABLE IF EXISTS a".to_string(),
            "DROP TABLE IF EXISTS b".to_string()
        ]
    );
}

#[test]
fn ddl_rename_table() {
    same(
        &schema_part(&split("RENAME TABLE a TO b", &cx()))[0],
        "ALTER TABLE a RENAME TO b",
    );
}

#[test]
fn mysql_type_display() {
    for (input, display) in [
        ("bigint(20) unsigned", "bigint unsigned"),
        ("int(11)", "int"),
        ("INTEGER", "int"),
        ("tinyint(1)", "tinyint(1)"),
        ("varchar(191)", "varchar(191)"),
        ("decimal(10,2)", "decimal(10,2)"),
        ("enum('a','b')", "enum('a','b')"),
        ("longtext", "longtext"),
        ("double", "double"),
        ("datetime", "datetime"),
        ("int(10) unsigned zerofill", "int(10) unsigned zerofill"),
    ] {
        assert_eq!(
            MySqlType::parse(input).unwrap().display(),
            display,
            "{input}"
        );
    }
    for (input, default) in [
        ("bigint", "0"),
        ("varchar(20)", ""),
        ("datetime", "0000-00-00 00:00:00"),
        ("enum('x','y')", "x"),
        ("date", "0000-00-00"),
        ("year", "0000"),
    ] {
        assert_eq!(
            MySqlType::parse(input).unwrap().implicit_default(),
            default,
            "{input}"
        );
    }
}

#[test]
fn catalogue_rows_for_wp_options() {
    let writes = catalogue_part(&split(WP_OPTIONS, &cx()));
    for row in [
        "('wp_options', 1, 'option_id', 'bigint unsigned', 0, NULL, 'auto_increment', NULL)",
        "('wp_options', 2, 'option_name', 'varchar(191)', 0, '', '', 'utf8mb4_unicode_520_ci')",
        "('wp_options', 'PRIMARY', 'PRIMARY', 1, 'option_id', NULL, (SELECT COALESCE(MAX(created), 0) + 1 FROM _speedyspec_indexes WHERE table_name = 'wp_options'))",
        "('wp_options', 'option_name', 'UNIQUE', 1, 'option_name', NULL, (SELECT COALESCE(MAX(created), 0) + 1 FROM _speedyspec_indexes WHERE table_name = 'wp_options'))",
        "('wp_options', 'autoload', 'INDEX', 1, 'autoload', NULL, (SELECT COALESCE(MAX(created), 0) + 1 FROM _speedyspec_indexes WHERE table_name = 'wp_options'))",
    ] {
        assert!(writes.contains(row), "{row} in\n{writes}");
    }
    assert!(writes.starts_with("DELETE FROM _speedyspec_"), "{writes}");
}

#[test]
fn column_key_rules() {
    let index = |name: &str, kind: IndexKind, cols: &[&str]| CatalogueIndex {
        name: name.into(),
        kind,
        parts: cols.iter().map(|c| (c.to_string(), None)).collect(),
    };
    let indexes = vec![
        index("PRIMARY", IndexKind::Primary, &["id"]),
        index("u1", IndexKind::Unique, &["a"]),
        index("u2", IndexKind::Unique, &["b", "c"]),
        index("k", IndexKind::Index, &["d", "e"]),
    ];
    assert_eq!(column_key("id", &indexes), "PRI");
    assert_eq!(column_key("a", &indexes), "UNI");
    assert_eq!(column_key("b", &indexes), "MUL");
    assert_eq!(column_key("e", &indexes), "");
    assert_eq!(column_key("d", &indexes), "MUL");
}

#[test]
fn unnamed_keys_named_like_mysql() {
    let table = catalogue_table(
        parse_one("CREATE TABLE t (a int, b int, KEY (a), KEY (a, b), UNIQUE (b))")
            .unwrap()
            .tree(),
    )
    .unwrap();
    let names: Vec<&str> = table.indexes.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["a", "a_2", "b"]);
}

#[test]
fn table_charset_default_collation() {
    let table = catalogue_table(
        parse_one("CREATE TABLE t (a varchar(5), n int) DEFAULT CHARSET=utf8mb4")
            .unwrap()
            .tree(),
    )
    .unwrap();
    assert_eq!(
        table.columns[0].collation.as_deref(),
        Some("utf8mb4_0900_ai_ci")
    );
    assert_eq!(table.columns[1].collation, None);
}

#[test]
fn fulltext_only_in_catalogue() {
    let stmts = split(
        "CREATE TABLE t (id int, body text, PRIMARY KEY (id), FULLTEXT KEY body (body))",
        &cx(),
    );
    assert_eq!(schema_part(&stmts).len(), 1);
    assert!(catalogue_part(&stmts).contains("'FULLTEXT'"));
}

#[test]
fn ddl_refusals() {
    for (sql, construct) in [
        ("CREATE TABLE a LIKE b", "CREATE TABLE … LIKE"),
        (
            "CREATE TABLE t (id int) AUTO_INCREMENT=100",
            "the AUTO_INCREMENT=n table option",
        ),
        ("CREATE TABLE t AS SELECT 1 AS a", "CREATE TABLE … SELECT"),
    ] {
        match rewrite_one(DdlPass, sql, &cx()) {
            Err(DbError::Unsupported {
                construct: c,
                dialect: DialectKind::Sqlite,
            }) => assert_eq!(c, construct, "{sql}"),
            other => panic!("{sql}: {other:?}"),
        }
    }
}

#[test]
fn ddl_parser_gaps() {
    for sql in [
        "ALTER TABLE t ENGINE=InnoDB",
        "ALTER TABLE t DEFAULT CHARSET utf8mb4",
        "ALTER TABLE t CONVERT TO CHARACTER SET utf8mb4",
        "ALTER TABLE t DROP KEY k",
        "ALTER TABLE t RENAME x",
    ] {
        assert!(
            matches!(parse_one(sql), Err(DbError::Parse { .. })),
            "{sql}"
        );
    }
}

#[test]
fn create_if_not_exists_existing_is_noop() {
    let schema = StaticIntrospector::from_mysql_ddl(&["CREATE TABLE t (id int)"]);
    let cx = PassContext {
        target: DialectKind::Sqlite,
        introspector: Some(&schema),
    };
    assert_eq!(
        rewrite_one(DdlPass, "CREATE TABLE IF NOT EXISTS t (id int)", &cx).unwrap(),
        Rewritten::Emulated(EmulatedOp::Noop)
    );
    assert!(matches!(
        rewrite_one(DdlPass, "CREATE TABLE IF NOT EXISTS u (id int)", &cx).unwrap(),
        Rewritten::Statements(_)
    ));
}

#[test]
fn catalogue_tables_pass_through() {
    let stmts = split(
        "CREATE TABLE IF NOT EXISTS _speedyspec_tables (table_name varchar(191) NOT NULL)",
        &cx(),
    );
    assert_eq!(stmts.len(), 1);
}

#[test]
fn catalogue_ddl_parses_and_passes_through() {
    let ddl = speedyspec_data_translate::catalogue_ddl();
    assert_eq!(ddl.len(), 3);
    for stmt in ddl {
        let sql = stmt.tree().to_string();
        assert_eq!(split(&sql, &cx()).len(), 1, "{sql}");
    }
}

#[test]
fn ddl_binary_table_collation_moves_to_columns() {
    let stmts = split(
        "CREATE TABLE t (a varchar(10), b int, c text COLLATE utf8mb4_unicode_ci) COLLATE utf8mb4_bin",
        &cx(),
    );
    same(
        &schema_part(&stmts)[0],
        "CREATE TABLE t (a varchar(10) COLLATE utf8mb4_bin, b int, c text COLLATE utf8mb4_unicode_ci)",
    );
}
