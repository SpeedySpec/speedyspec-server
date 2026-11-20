//! Data-access contracts for speedyspec.
//!
//! Values, rows and the statement model (Query Object); the SQL, NoSQL and Direct store contracts; dialects
//! (Strategy); engines (Abstract Factory); and the Repository and Unit of Work contracts. No implementations live
//! here.

pub use sqlparser::ast;

mod dialect;
mod engine;
mod error;
mod row;
mod statement;
mod store;
mod uow;
mod value;

pub use dialect::{Capabilities, Dialect, EmulatedOp, Prepared, UpsertForm};
pub use engine::{Engine, EngineConfig, EngineFactory, EngineHandle, EngineKind, EngineLookup};
pub use error::{DbError, DialectKind};
pub use row::{ColumnInfo, ExecResult, Row, Rows};
pub use statement::{EmittedSql, Statement, StatementKind, TxControl};
pub use store::{
    ColumnDef, DirectStore, Health, IndexDef, Introspector, KeyValues, NoSqlStore, SqlStore,
};
pub use uow::{
    ChangeKind, CommitError, CommitReport, Identified, PendingChange, Repository, UnitOfWork,
};
pub use value::DbValue;
