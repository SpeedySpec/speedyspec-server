//! Shared by the PostgreSQL tests: a dialect over WordPress's tables, and SQL compared as PostgreSQL reads it.

#![allow(dead_code)]

use std::ops::ControlFlow;
use std::rc::Rc;

use speedyspec_data::{DbValue, Introspector, Statement};
use speedyspec_data_postgres::{PgEmitted, PgPlan, PostgresDialect};
use speedyspec_data_query::parse_one;
use speedyspec_data_translate::testing::StaticIntrospector;
use sqlparser::ast::{Ident, VisitMut, VisitorMut};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

pub const WP_OPTIONS: &str = "CREATE TABLE wp_options (option_id bigint(20) unsigned NOT NULL auto_increment, option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), UNIQUE KEY option_name (option_name), KEY autoload (autoload)) DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_520_ci";
pub const WP_POSTS: &str = "CREATE TABLE wp_posts (ID bigint(20) unsigned NOT NULL auto_increment, post_date datetime NOT NULL default '0000-00-00 00:00:00', post_date_gmt datetime NOT NULL default '0000-00-00 00:00:00', post_title text NOT NULL, post_status varchar(20) NOT NULL default 'publish', post_type varchar(20) NOT NULL default 'post', menu_order int(11) NOT NULL default '0', PRIMARY KEY (ID)) DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_520_ci";
pub const WP_POSTMETA: &str = "CREATE TABLE wp_postmeta (meta_id bigint(20) unsigned NOT NULL auto_increment, post_id bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, meta_value longtext, PRIMARY KEY (meta_id), KEY post_id (post_id), KEY meta_key (meta_key(191))) DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_520_ci";
pub const T_TWO_UNIQUE: &str = "CREATE TABLE t_two_unique (id bigint NOT NULL auto_increment, name varchar(50) NOT NULL, n int, PRIMARY KEY (id), UNIQUE KEY name (name))";

/// A dialect over WordPress's options, posts and postmeta tables, and `t_two_unique`.
pub fn wp_dialect() -> PostgresDialect {
    let introspector: Rc<dyn Introspector> = Rc::new(StaticIntrospector::from_mysql_ddl(&[
        WP_OPTIONS,
        WP_POSTS,
        WP_POSTMETA,
        T_TWO_UNIQUE,
    ]));
    PostgresDialect::new(Some(introspector))
}

/// The live test server.
pub fn pg_url() -> String {
    std::env::var("SPEEDYSPEC_TEST_POSTGRES_URL").expect("set SPEEDYSPEC_TEST_POSTGRES_URL")
}

struct Lower;

impl VisitorMut for Lower {
    type Break = ();

    fn pre_visit_ident(&mut self, ident: &mut Ident) -> ControlFlow<()> {
        if ident.quote_style.is_none() {
            ident.value = ident.value.to_lowercase();
        }
        ControlFlow::Continue(())
    }
}

fn normal(sql: &str) -> String {
    let mut parsed =
        Parser::parse_sql(&PostgreSqlDialect {}, sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
    for stmt in &mut parsed {
        let _ = stmt.visit(&mut Lower);
    }
    parsed
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// Plans `mysql` with `params` and checks each emitted statement against `expected`, as PostgreSQL reads them.
pub fn assert_pg(
    d: &PostgresDialect,
    mysql: &str,
    params: Vec<DbValue>,
    expected: &[&str],
) -> Vec<PgEmitted> {
    let (tree, _) = parse_one(mysql)
        .unwrap_or_else(|e| panic!("{mysql}: {e:?}"))
        .into_parts();
    let stmt = Statement::new(tree, params);
    let emitted = match d.plan(&stmt).unwrap_or_else(|e| panic!("{mysql}: {e:?}")) {
        PgPlan::Sql(emitted) => emitted,
        other => panic!("{mysql}: expected SQL, got {other:?}"),
    };
    let actual: Vec<&str> = emitted.iter().map(|e| e.sql.sql.as_str()).collect();
    assert_eq!(actual.len(), expected.len(), "{mysql}: {actual:?}");
    for (a, e) in actual.iter().zip(expected) {
        assert_eq!(
            normal(a),
            normal(e),
            "\nmysql:    {mysql}\nactual:   {a}\nexpected: {e}"
        );
    }
    emitted
}
