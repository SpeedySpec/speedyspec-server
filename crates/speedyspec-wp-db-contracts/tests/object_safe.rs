//! The wpdb port is object safe: a seam holds it as `Rc<dyn Wpdb>`.

use std::rc::Rc;

use speedyspec_data::Statement;
use speedyspec_wp_db_contracts::{Field, QueryOutcome, WpDbError, WpValue, Wpdb};

/// Answers table names with the `wp_` prefix, and nothing else.
struct Prefixed;

impl Wpdb for Prefixed {
    fn table_name(&self, logical: &str) -> String {
        format!("wp_{logical}")
    }

    fn query_statement(&self, _stmt: Statement) -> Result<QueryOutcome, WpDbError> {
        Ok(QueryOutcome::Done)
    }

    fn query_bytes(&self, _sql: &[u8]) -> Result<QueryOutcome, WpDbError> {
        Ok(QueryOutcome::Done)
    }

    fn prepare(&self, query: Option<&[u8]>, _args: &[WpValue]) -> Option<Vec<u8>> {
        query.map(<[u8]>::to_vec)
    }

    fn process_fields(
        &self,
        _table: &str,
        _data: &[(String, WpValue)],
        _format: Option<&[String]>,
    ) -> Result<Vec<Field>, WpDbError> {
        Ok(Vec::new())
    }

    fn insert(
        &self,
        _table: &str,
        _data: &[(String, WpValue)],
        _format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        Ok(0)
    }

    fn update(
        &self,
        _table: &str,
        _data: &[(String, WpValue)],
        _conditions: &[(String, WpValue)],
        _format: Option<&[String]>,
        _where_format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        Ok(0)
    }

    fn delete(
        &self,
        _table: &str,
        _conditions: &[(String, WpValue)],
        _where_format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        Ok(0)
    }

    fn insert_id(&self) -> u64 {
        0
    }

    fn suppress_errors(&self, _suppress: bool) -> bool {
        false
    }

    fn charset(&self) -> String {
        "utf8mb4".into()
    }
}

#[test]
fn wpdb_is_object_safe() {
    let db: Rc<dyn Wpdb> = Rc::new(Prefixed);
    assert_eq!(db.table_name("options"), "wp_options");
}
