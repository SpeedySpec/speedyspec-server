//! Data-access contracts for speedyspec.
//!
//! Values, rows and the statement model (Query Object). No implementations live here.

pub use sqlparser::ast;

mod error;
mod row;
mod statement;
mod value;

pub use error::{DbError, DialectKind};
pub use row::{ColumnInfo, ExecResult, Row, Rows};
pub use statement::{EmittedSql, Statement, StatementKind, TxControl};
pub use value::DbValue;
