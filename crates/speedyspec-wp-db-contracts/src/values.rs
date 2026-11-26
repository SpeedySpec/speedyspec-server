//! wpdb's values: what a query returns, its rows, and the fields its helpers build.

use std::rc::Rc;

use speedyspec_data::Row;
use speedyspec_hooks::Value;

/// A PHP value, as the Hook API carries it.
pub type WpValue = speedyspec_hooks::Value;

/// What `query()` returns: rows for a read, counts for a write, `true` for DDL.
#[derive(Debug, Clone, PartialEq)]
pub enum QueryOutcome {
    Rows(Rc<Vec<WpRow>>),
    Affected { rows: u64, insert_id: u64 },
    Done,
}

impl QueryOutcome {
    /// `query()`'s PHP return value.
    pub fn to_wp(&self) -> WpValue {
        match self {
            QueryOutcome::Rows(rows) => WpValue::Int(rows.len() as i64),
            QueryOutcome::Affected { rows, .. } => WpValue::Int(*rows as i64),
            QueryOutcome::Done => WpValue::Bool(true),
        }
    }
}

/// One row as `mysqli_fetch_object` then `get_object_vars` give it: each column name once, at its first position,
/// holding its last value; values are MySQL's text bytes or null.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WpRow {
    pub fields: Vec<(String, Option<Vec<u8>>)>,
}

impl WpRow {
    pub fn from_row(row: &Row) -> WpRow {
        let mut fields: Vec<(String, Option<Vec<u8>>)> = Vec::with_capacity(row.columns().len());
        for (name, value) in row.columns().iter().zip(row.values()) {
            let text = value.to_mysql_text();
            match fields.iter_mut().find(|(n, _)| n == name) {
                Some(existing) => existing.1 = text,
                None => fields.push((name.clone(), text)),
            }
        }
        WpRow { fields }
    }

    pub fn get(&self, name: &str) -> Option<&Option<Vec<u8>>> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
}

/// How a column's length is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthKind {
    Byte,
    Char,
}

/// A column's maximum length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColLength {
    pub kind: LengthKind,
    pub length: u64,
}

/// One field of a helper's data, as `process_fields` builds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub value: Value,
    pub format: String,
    pub charset: Option<String>,
    pub length: Option<ColLength>,
}
