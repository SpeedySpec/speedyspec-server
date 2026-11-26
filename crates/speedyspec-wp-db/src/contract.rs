//! `NativeWpdb` implements the `Wpdb` port. Each method names `NativeWpdb`'s own method in full, so none of them calls itself.

use speedyspec_data::Statement;
use speedyspec_wp_db_contracts::{Field, QueryOutcome, WpDbError, WpValue, Wpdb};

use crate::{NativeWpdb, TABLE_PROPERTIES};

impl Wpdb for NativeWpdb {
    /// `$wpdb->{logical}`. A declared table property answers its prefixed name even while unset; any other name
    /// is empty until it is set, as PHP reads an undefined property (`_get_meta_table()` relies on that).
    fn table_name(&self, logical: &str) -> String {
        match self.with_tables(|t| t.get(logical).map(str::to_string)) {
            Some(name) => name,
            None if TABLE_PROPERTIES.contains(&logical) => NativeWpdb::table_name(self, logical),
            None => String::new(),
        }
    }

    fn query_statement(&self, stmt: Statement) -> Result<QueryOutcome, WpDbError> {
        NativeWpdb::query_statement(self, stmt)
    }

    fn query_bytes(&self, sql: &[u8]) -> Result<QueryOutcome, WpDbError> {
        NativeWpdb::query_bytes(self, sql)
    }

    fn prepare(&self, query: Option<&[u8]>, args: &[WpValue]) -> Option<Vec<u8>> {
        NativeWpdb::prepare(self, query, args)
    }

    fn process_fields(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        format: Option<&[String]>,
    ) -> Result<Vec<Field>, WpDbError> {
        NativeWpdb::process_fields(self, table, data, format)
    }

    fn insert(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        NativeWpdb::insert(self, table, data, format)
    }

    fn update(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        conditions: &[(String, WpValue)],
        format: Option<&[String]>,
        where_format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        NativeWpdb::update(self, table, data, conditions, format, where_format)
    }

    fn delete(
        &self,
        table: &str,
        conditions: &[(String, WpValue)],
        where_format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        NativeWpdb::delete(self, table, conditions, where_format)
    }

    fn insert_id(&self) -> u64 {
        NativeWpdb::insert_id(self)
    }

    fn suppress_errors(&self, suppress: bool) -> bool {
        NativeWpdb::suppress_errors(self, suppress)
    }

    fn charset(&self) -> String {
        NativeWpdb::charset(self)
    }
}
