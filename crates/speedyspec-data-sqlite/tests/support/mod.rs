//! Shared by the SQLite translation tests: plan a statement against a schema, and compare SQL as SQLite reads it.

#![allow(dead_code)]

use speedyspec_data_query::parse_one;
use speedyspec_data_sqlite::{SqliteDialect, SqlitePlan, SqliteStep};
use speedyspec_data_translate::testing::StaticIntrospector;
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;

/// The steps SQLite runs for `sql` (MySQL text), with a schema described by `ddl`.
pub fn plan_run(sql: &str, ddl: &[&str]) -> Vec<SqliteStep> {
    let introspector = StaticIntrospector::from_mysql_ddl(ddl);
    let stmt = parse_one(sql).unwrap_or_else(|e| panic!("{sql}: {e:?}"));
    match SqliteDialect::new().plan(&stmt, Some(&introspector)) {
        Ok(SqlitePlan::Run { steps, .. }) => steps,
        other => panic!("{sql}: expected steps, got {other:?}"),
    }
}

/// Each step's SQL.
pub fn plan_steps(sql: &str, ddl: &[&str]) -> Vec<String> {
    plan_run(sql, ddl).into_iter().map(|s| s.sql.sql).collect()
}

/// `actual` and `expected` print alike once `sqlparser`'s SQLite dialect has parsed and printed them again.
pub fn assert_sqlite_sql(actual: &str, expected: &str) {
    let print = |sql: &str| {
        Parser::parse_sql(&SQLiteDialect {}, sql)
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    };
    assert_eq!(
        print(actual),
        print(expected),
        "\nactual:   {actual}\nexpected: {expected}"
    );
}

/// Whether a step writes the MySQL schema catalogue.
pub fn is_catalogue_write(sql: &str) -> bool {
    sql.contains("_speedyspec_")
}
