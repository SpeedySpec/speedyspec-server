//! MySQL-to-portable rewrite passes shared by the SQLite and PostgreSQL engines, and the MySQL knowledge they need.
//!
//! Every pass is a Visitor over `sqlparser`'s tree, run as a link in a `PassChain` (Chain of Responsibility). Params
//! are numbered first ([`number_placeholders`]), so a pass may move or repeat expressions without touching them.

mod expressions;
mod hints;
mod lexical;
mod params;

use speedyspec_data_query::PassChain;

pub use expressions::{ExpressionPass, function_call};
pub use hints::HintsPass;
pub use lexical::LexicalPass;
pub use params::number_placeholders;

/// The neutral passes, in order: administrative statements, DDL, lexical, hints, expressions, inserts.
pub fn neutral_chain() -> PassChain {
    let mut chain = PassChain::new();
    chain.push(Box::new(LexicalPass));
    chain.push(Box::new(HintsPass));
    chain.push(Box::new(ExpressionPass));
    chain
}
