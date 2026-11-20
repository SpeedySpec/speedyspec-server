//! Dialects (Strategy): how a statement becomes text one engine runs.

use crate::{DbError, DialectKind, EmittedSql, Statement};

/// How an engine writes an upsert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertForm {
    OnDuplicateKey,
    OnConflict,
    None,
}

/// What a store can do; callers check it instead of the engine's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub transactions: bool,
    pub savepoints: bool,
    pub ordered_ranges: bool,
    pub expiry: bool,
    pub atomic_increment: bool,
    pub returning: bool,
    pub upsert: UpsertForm,
}

/// A MySQL statement an engine answers itself instead of running SQL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmulatedOp {
    ShowTables {
        like: Option<String>,
    },
    ShowColumns {
        table: String,
        full: bool,
        like: Option<String>,
    },
    ShowIndex {
        table: String,
    },
    ShowCreateTable {
        table: String,
    },
    ShowVariables {
        like: Option<String>,
    },
    ShowTableStatus {
        like: Option<String>,
    },
    FoundRows,
    SetSession {
        name: String,
        value: String,
    },
    Noop,
}

/// A statement made ready for one engine.
#[derive(Debug, Clone, PartialEq)]
pub enum Prepared {
    Sql(Vec<EmittedSql>),
    Emulated(EmulatedOp),
}

/// Turns a parsed statement into what one engine runs; a construct it cannot express is `Unsupported`, never a silent rewrite.
pub trait Dialect {
    fn kind(&self) -> DialectKind;
    fn prepare(&self, stmt: &Statement) -> Result<Prepared, DbError>;
}
