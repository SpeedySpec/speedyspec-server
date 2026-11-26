//! `WpDb` implements the `Wpdb` port. Each method names `WpDb`'s own method in full, so none of them calls itself.

use speedyspec_data::Statement;
use speedyspec_wp_db_contracts::{Field, QueryOutcome, WpDbError, WpValue, Wpdb};

use crate::WpDb;

impl Wpdb for WpDb {
    fn table_name(&self, logical: &str) -> String {
        WpDb::table_name(self, logical)
    }

    fn query_statement(&self, stmt: Statement) -> Result<QueryOutcome, WpDbError> {
        WpDb::query_statement(self, stmt)
    }

    fn query_bytes(&self, sql: &[u8]) -> Result<QueryOutcome, WpDbError> {
        WpDb::query_bytes(self, sql)
    }

    fn prepare(&self, query: Option<&[u8]>, args: &[WpValue]) -> Option<Vec<u8>> {
        WpDb::prepare(self, query, args)
    }

    fn process_fields(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        format: Option<&[String]>,
    ) -> Result<Vec<Field>, WpDbError> {
        WpDb::process_fields(self, table, data, format)
    }

    fn insert(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        WpDb::insert(self, table, data, format)
    }

    fn update(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        conditions: &[(String, WpValue)],
        format: Option<&[String]>,
        where_format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        WpDb::update(self, table, data, conditions, format, where_format)
    }

    fn delete(
        &self,
        table: &str,
        conditions: &[(String, WpValue)],
        where_format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        WpDb::delete(self, table, conditions, where_format)
    }

    fn insert_id(&self) -> u64 {
        WpDb::insert_id(self)
    }

    fn suppress_errors(&self, suppress: bool) -> bool {
        WpDb::suppress_errors(self, suppress)
    }

    fn charset(&self) -> String {
        WpDb::charset(self)
    }
}
