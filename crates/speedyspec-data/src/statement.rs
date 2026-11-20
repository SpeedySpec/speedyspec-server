//! The statement model: a parsed syntax tree and its bound parameters (Query Object).

use sqlparser::ast;

use crate::DbValue;

/// What a statement does, read from its tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    Select,
    Insert,
    Replace,
    Update,
    Delete,
    Ddl,
    Show,
    Set,
    Transaction(TxControl),
    Other,
}

/// Transaction-control statements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxControl {
    Begin,
    Commit,
    Rollback,
    Savepoint,
    Release,
    RollbackTo,
}

/// A parsed statement. Placeholders in the tree are `?`, one per param, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    tree: ast::Statement,
    params: Vec<DbValue>,
}

impl Statement {
    pub fn new(tree: ast::Statement, params: Vec<DbValue>) -> Statement {
        Statement { tree, params }
    }

    pub fn tree(&self) -> &ast::Statement {
        &self.tree
    }

    pub fn tree_mut(&mut self) -> &mut ast::Statement {
        &mut self.tree
    }

    pub fn params(&self) -> &[DbValue] {
        &self.params
    }

    pub fn params_mut(&mut self) -> &mut Vec<DbValue> {
        &mut self.params
    }

    pub fn into_parts(self) -> (ast::Statement, Vec<DbValue>) {
        (self.tree, self.params)
    }

    pub fn kind(&self) -> StatementKind {
        use ast::Statement as S;
        match &self.tree {
            S::Query(_) => StatementKind::Select,
            S::Insert(insert) if insert.replace_into => StatementKind::Replace,
            S::Insert(_) => StatementKind::Insert,
            S::Update(_) => StatementKind::Update,
            S::Delete(_) => StatementKind::Delete,
            S::CreateTable(_)
            | S::CreateIndex(_)
            | S::CreateView(_)
            | S::CreateDatabase { .. }
            | S::CreateSchema { .. }
            | S::CreateFunction(_)
            | S::CreateTrigger(_)
            | S::CreateProcedure { .. }
            | S::CreateSequence { .. }
            | S::AlterTable(_)
            | S::AlterIndex { .. }
            | S::AlterView(_)
            | S::Drop { .. }
            | S::DropFunction(_)
            | S::DropTrigger(_)
            | S::DropProcedure { .. }
            | S::Truncate(_)
            | S::RenameTable(_) => StatementKind::Ddl,
            S::ShowFunctions { .. }
            | S::ShowVariable { .. }
            | S::ShowStatus { .. }
            | S::ShowVariables { .. }
            | S::ShowCreate { .. }
            | S::ShowColumns { .. }
            | S::ShowCatalogs { .. }
            | S::ShowDatabases { .. }
            | S::ShowProcessList { .. }
            | S::ShowSchemas { .. }
            | S::ShowCharset(_)
            | S::ShowObjects(_)
            | S::ShowTables { .. }
            | S::ShowViews { .. }
            | S::ShowCollation { .. } => StatementKind::Show,
            S::Set(_) => StatementKind::Set,
            S::StartTransaction { .. } => StatementKind::Transaction(TxControl::Begin),
            S::Commit { .. } => StatementKind::Transaction(TxControl::Commit),
            S::Rollback { savepoint: Some(_), .. } => {
                StatementKind::Transaction(TxControl::RollbackTo)
            }
            S::Rollback { .. } => StatementKind::Transaction(TxControl::Rollback),
            S::Savepoint { .. } => StatementKind::Transaction(TxControl::Savepoint),
            S::ReleaseSavepoint { .. } => StatementKind::Transaction(TxControl::Release),
            _ => StatementKind::Other,
        }
    }
}

/// SQL text an engine runs, with its parameters, produced by a dialect from a `Statement`.
#[derive(Debug, Clone, PartialEq)]
pub struct EmittedSql {
    pub sql: String,
    pub params: Vec<DbValue>,
}
