//! The SQLite passes, run after the neutral ones (Chain of Responsibility, each a Visitor over the tree).

mod alter;
mod column_types;
mod dml;
mod functions;
mod limit;
mod numeric_compare;
mod quote;

use speedyspec_data_query::PassChain;
use speedyspec_data_translate::{RowLimitPass, neutral_chain};

pub use alter::{REBUILD_SUFFIX, SqliteAlter};
pub use column_types::SqliteTypes;
pub use dml::SqliteDml;
pub use functions::SqliteFunctions;
pub use limit::SqliteLimit;
pub use numeric_compare::SqliteNumericCompare;
pub use quote::SqliteQuote;

/// The neutral passes, then SQLite's, in order; quoting is last.
pub fn sqlite_chain() -> PassChain {
    let mut chain = neutral_chain();
    chain.push(Box::new(RowLimitPass::new("rowid")));
    chain.push(Box::new(SqliteAlter));
    chain.push(Box::new(SqliteTypes));
    chain.push(Box::new(SqliteDml));
    chain.push(Box::new(SqliteLimit));
    chain.push(Box::new(SqliteNumericCompare));
    chain.push(Box::new(SqliteFunctions));
    chain.push(Box::new(SqliteQuote));
    chain
}
