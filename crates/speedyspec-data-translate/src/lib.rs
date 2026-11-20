//! MySQL-to-portable rewrite passes shared by the SQLite and PostgreSQL engines, and the MySQL knowledge they need.
//!
//! Every pass is a Visitor over `sqlparser`'s tree, run as a link in a `PassChain` (Chain of Responsibility). Params
//! are numbered first ([`number_placeholders`]), so a pass may move or repeat expressions without touching them.

mod admin;
mod expressions;
mod found_rows;
mod hints;
mod insert;
mod lexical;
mod names;
mod params;
mod row_limit;

use speedyspec_data_query::PassChain;

pub use admin::AdminPass;
pub use expressions::{ExpressionPass, function_call};
pub use found_rows::take_found_rows;
pub use hints::HintsPass;
pub use insert::InsertPass;
pub use lexical::LexicalPass;
pub use params::number_placeholders;
pub use row_limit::RowLimitPass;

/// The neutral passes, in order: administrative statements, DDL, lexical, hints, expressions, inserts.
pub fn neutral_chain() -> PassChain {
    let mut chain = PassChain::new();
    chain.push(Box::new(AdminPass));
    chain.push(Box::new(LexicalPass));
    chain.push(Box::new(HintsPass));
    chain.push(Box::new(ExpressionPass));
    chain.push(Box::new(InsertPass));
    chain
}
