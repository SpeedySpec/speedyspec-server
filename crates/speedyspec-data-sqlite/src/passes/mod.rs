//! The SQLite passes, run after the neutral ones (Chain of Responsibility, each a Visitor over the tree).

mod alter;
mod column_types;
mod quote;

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::{PassChain, PassContext, Rewrite, RewritePass};
use speedyspec_data_translate::{RowLimitPass, neutral_chain};

pub use alter::{REBUILD_SUFFIX, SqliteAlter};
pub use column_types::SqliteTypes;
pub use quote::SqliteQuote;

/// A pass whose work comes in a later task; it holds its place in the chain.
struct Placeholder(&'static str);

impl RewritePass for Placeholder {
    fn name(&self) -> &'static str {
        self.0
    }

    fn rewrite(&self, _stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        Ok(Rewrite::Continue)
    }
}

/// The neutral passes, then SQLite's, in order; quoting is last.
pub fn sqlite_chain() -> PassChain {
    let mut chain = neutral_chain();
    chain.push(Box::new(RowLimitPass::new("rowid")));
    chain.push(Box::new(SqliteAlter));
    chain.push(Box::new(SqliteTypes));
    chain.push(Box::new(Placeholder("sqlite_dml")));
    chain.push(Box::new(Placeholder("sqlite_numeric_compare")));
    chain.push(Box::new(Placeholder("sqlite_functions")));
    chain.push(Box::new(SqliteQuote));
    chain
}
