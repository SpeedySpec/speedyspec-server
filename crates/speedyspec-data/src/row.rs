//! Result rows and column metadata.

use std::rc::Rc;

use crate::DbValue;

/// A column's metadata, as mysqli's field info reports it (`wpdb::get_col_info` exposes it).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColumnInfo {
    pub name: String,
    pub org_name: String,
    pub table: String,
    pub org_table: String,
    pub sql_type: String,
    pub length: u64,
    pub max_length: u64,
    pub nullable: bool,
    pub flags: u32,
    pub decimals: u8,
    pub charset: u16,
}

/// One result row; column names are shared by every row of a result.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    columns: Rc<[String]>,
    values: Vec<DbValue>,
}

impl Row {
    pub fn new(columns: Rc<[String]>, values: Vec<DbValue>) -> Row {
        Row { columns, values }
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    pub fn values(&self) -> &[DbValue] {
        &self.values
    }

    /// The value of the last column with this name, as mysqli's associative fetch keeps the last duplicate.
    pub fn get(&self, column: &str) -> Option<&DbValue> {
        self.columns
            .iter()
            .rposition(|c| c == column)
            .and_then(|i| self.values.get(i))
    }

    pub fn get_index(&self, i: usize) -> Option<&DbValue> {
        self.values.get(i)
    }
}

/// A query's result: column metadata and rows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rows {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Row>,
}

/// The outcome of a statement that returns no rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExecResult {
    pub rows_affected: u64,
    pub insert_id: Option<u64>,
}
