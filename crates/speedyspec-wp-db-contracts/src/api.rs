//! The wpdb port: `wpdb`, as other seams call it.

use speedyspec_data::Statement;

use crate::{Field, QueryOutcome, WpDbError, WpValue};

/// wpdb as other seams call it (Gateway; an API port). Each method is the wpdb method of the same name.
pub trait Wpdb {
    /// `$wpdb->options`, `$wpdb->postmeta`: a logical table's prefixed name.
    fn table_name(&self, logical: &str) -> String;
    /// `wpdb::query` for a parsed statement, as ported APIs send it.
    fn query_statement(&self, stmt: Statement) -> Result<QueryOutcome, WpDbError>;
    /// `wpdb::query` for query text that may not be UTF-8 (PHP strings).
    fn query_bytes(&self, sql: &[u8]) -> Result<QueryOutcome, WpDbError>;
    /// `wpdb::prepare`.
    fn prepare(&self, query: Option<&[u8]>, args: &[WpValue]) -> Option<Vec<u8>>;
    /// `wpdb::process_fields`.
    fn process_fields(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        format: Option<&[String]>,
    ) -> Result<Vec<Field>, WpDbError>;
    /// `wpdb::insert`.
    fn insert(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        format: Option<&[String]>,
    ) -> Result<u64, WpDbError>;
    /// `wpdb::update`.
    fn update(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        conditions: &[(String, WpValue)],
        format: Option<&[String]>,
        where_format: Option<&[String]>,
    ) -> Result<u64, WpDbError>;
    /// `wpdb::delete`; phase 3 (`delete_metadata_by_mid()`).
    fn delete(
        &self,
        table: &str,
        conditions: &[(String, WpValue)],
        where_format: Option<&[String]>,
    ) -> Result<u64, WpDbError>;
    /// `$wpdb->insert_id`; phase 3 (`add_metadata()`).
    fn insert_id(&self) -> u64;
    /// `wpdb::suppress_errors`: sets the flag and returns the previous one.
    fn suppress_errors(&self, suppress: bool) -> bool;
    /// `$wpdb->charset`.
    fn charset(&self) -> String;
}
