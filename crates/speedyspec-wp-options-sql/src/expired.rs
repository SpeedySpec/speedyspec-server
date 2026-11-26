//! `SqlExpiredTransients`: `delete_expired_transients()`'s multi-table `DELETE`s, WordPress's text with its three
//! values bound, sent through `WpDb` so the `query` filter fires.

use std::rc::Rc;

use speedyspec_data::{DbError, DbValue, Statement};
use speedyspec_data_query::parse_one;
use speedyspec_wp_db::WpDb;
use speedyspec_wp_db_contracts::{QueryOutcome, WpDbError};
use speedyspec_wp_options::{ExpiredTransients, TransientPrefix};

pub struct SqlExpiredTransients {
    wpdb: Rc<WpDb>,
}

/// `$wpdb->esc_like( $text ) . '%'`.
fn like_prefix(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    for c in text.chars() {
        if matches!(c, '_' | '%' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

impl SqlExpiredTransients {
    pub fn new(wpdb: Rc<WpDb>) -> SqlExpiredTransients {
        SqlExpiredTransients { wpdb }
    }

    /// WordPress's statement over `table`'s `name`/`value` columns for `prefix`, `now` bound.
    fn purge(
        &self,
        table: &str,
        name: &str,
        value: &str,
        prefix: TransientPrefix,
        now: i64,
    ) -> Result<u64, DbError> {
        let timeout = prefix.timeout();
        // SUBSTRING is 1-based: the name after the prefix starts one past its length.
        let start = prefix.value().len() + 1;
        let sql = format!(
            "DELETE a, b FROM {table} a, {table} b WHERE a.{name} LIKE ? AND a.{name} NOT LIKE ? \
             AND b.{name} = CONCAT( '{timeout}', SUBSTRING( a.{name}, {start} ) ) AND b.{value} < ?"
        );
        let tree = parse_one(&sql)?.into_parts().0;
        let params = vec![
            DbValue::Text(like_prefix(prefix.value())),
            DbValue::Text(like_prefix(timeout)),
            DbValue::Int(now),
        ];
        match self.wpdb.query_statement(Statement::new(tree, params)) {
            Ok(QueryOutcome::Affected { rows, .. }) => Ok(rows),
            Ok(_) => Ok(0),
            Err(WpDbError::Hook(e)) => Err(DbError::Engine {
                engine: "wpdb".into(),
                code: Some("hook".into()),
                message: e.to_string(),
            }),
            // `$wpdb->query` returns false; `delete_expired_transients()` ignores it.
            Err(_) => Ok(0),
        }
    }
}

impl ExpiredTransients for SqlExpiredTransients {
    fn purge_options(&self, prefix: TransientPrefix, now: i64) -> Result<u64, DbError> {
        let table = self.wpdb.table_name("options");
        self.purge(&table, "option_name", "option_value", prefix, now)
    }

    fn purge_sitemeta(&self, now: i64) -> Result<u64, DbError> {
        let table = self.wpdb.table_name("sitemeta");
        self.purge(
            &table,
            "meta_key",
            "meta_value",
            TransientPrefix::SiteTransient,
            now,
        )
    }
}
