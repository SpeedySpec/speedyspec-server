//! The parser front end (Interpreter): SQL text into statements, with MySQL's dialect.

use speedyspec_data::{DbError, Statement};
use sqlparser::ast::Spanned;
use sqlparser::dialect::MySqlDialect;
use sqlparser::parser::{Parser, ParserError};

/// Parses exactly one statement, as mysqli without multi-statements accepts.
pub fn parse_one(sql: &str) -> Result<Statement, DbError> {
    let mut statements = parse_many(sql)?;
    match statements.len() {
        0 => Err(empty()),
        1 => Ok(statements.remove(0)),
        _ => {
            let start = statements[1].tree().span().start;
            Err(DbError::Parse {
                message: "multiple statements in one query".into(),
                line: start.line,
                column: start.column,
            })
        }
    }
}

/// Parses a batch of statements separated by `;` (for `dbDelta`-style batches).
pub fn parse_many(sql: &str) -> Result<Vec<Statement>, DbError> {
    let trees = Parser::parse_sql(&MySqlDialect {}, sql)
        .map_err(|e| at_end_if_unplaced(parse_error(e), sql))?;
    if trees.is_empty() {
        return Err(empty());
    }
    Ok(trees
        .into_iter()
        .map(|tree| Statement::new(tree, Vec::new()))
        .collect())
}

fn empty() -> DbError {
    DbError::Parse {
        message: "empty query".into(),
        line: 0,
        column: 0,
    }
}

/// Maps a `sqlparser` error, reading `Line: N, Column: M` from its message when present.
pub(crate) fn parse_error(error: ParserError) -> DbError {
    let message = match error {
        ParserError::TokenizerError(m) | ParserError::ParserError(m) => m,
        ParserError::RecursionLimitExceeded => "recursion limit exceeded".into(),
    };
    let (line, column) = position(&message).unwrap_or((0, 0));
    // The position is carried apart; sqlparser's " at Line: N, Column: M" suffix would repeat it.
    let message = match message.rfind(" at Line: ") {
        Some(at) if line > 0 => message[..at].to_string(),
        _ => message,
    };
    DbError::Parse {
        message,
        line,
        column,
    }
}

/// An error sqlparser reports without a position (it ran out of input) is placed just past the last character.
pub(crate) fn at_end_if_unplaced(error: DbError, sql: &str) -> DbError {
    match error {
        DbError::Parse {
            message,
            line: 0,
            column: 0,
        } => {
            let line = sql.matches('\n').count() as u64 + 1;
            let last = sql.rsplit('\n').next().unwrap_or("");
            DbError::Parse {
                message,
                line,
                column: last.chars().count() as u64 + 1,
            }
        }
        other => other,
    }
}

fn position(message: &str) -> Option<(u64, u64)> {
    let after = |key: &str| -> Option<u64> {
        let start = message.rfind(key)? + key.len();
        let digits: String = message[start..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    };
    Some((after("Line: ")?, after("Column: ")?))
}
