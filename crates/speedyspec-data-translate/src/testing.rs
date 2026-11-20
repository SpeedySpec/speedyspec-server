//! A schema held in memory, for tests of passes that consult the engine's schema.

use speedyspec_data::{ColumnDef, DbError, IndexDef, Introspector};
use speedyspec_data_query::parse_one;

use crate::{CatalogueTable, catalogue_table};

/// An `Introspector` over tables described by MySQL `CREATE TABLE` statements.
pub struct StaticIntrospector {
    tables: Vec<CatalogueTable>,
}

impl StaticIntrospector {
    pub fn from_mysql_ddl(ddl: &[&str]) -> StaticIntrospector {
        let tables = ddl
            .iter()
            .map(|sql| {
                catalogue_table(parse_one(sql).expect("test DDL parses").tree())
                    .expect("test DDL is a CREATE TABLE")
            })
            .collect();
        StaticIntrospector { tables }
    }

    fn table(&self, name: &str) -> Option<&CatalogueTable> {
        self.tables
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
    }
}

impl Introspector for StaticIntrospector {
    fn server_version(&self) -> Result<String, DbError> {
        Ok("8.4.0".into())
    }

    fn tables(&self, like: Option<&str>) -> Result<Vec<String>, DbError> {
        Ok(self
            .tables
            .iter()
            .map(|t| t.name.clone())
            .filter(|n| like.is_none_or(|l| l == n))
            .collect())
    }

    fn columns(&self, table: &str) -> Result<Vec<ColumnDef>, DbError> {
        Ok(self
            .table(table)
            .map(CatalogueTable::column_defs)
            .unwrap_or_default())
    }

    fn indexes(&self, table: &str) -> Result<Vec<IndexDef>, DbError> {
        Ok(self
            .table(table)
            .map(CatalogueTable::index_defs)
            .unwrap_or_default())
    }

    fn table_collation(&self, table: &str) -> Result<Option<String>, DbError> {
        Ok(self.table(table).and_then(|t| t.collation.clone()))
    }

    fn index_type(&self, table: &str, index: &str) -> Result<Option<String>, DbError> {
        Ok(self.table(table).and_then(|t| t.index_type(index)))
    }
}
