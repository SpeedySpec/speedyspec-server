//! The PostgreSQL passes, run after the neutral ones (Chain of Responsibility, each a Visitor over the tree).

mod identifiers;
mod placeholders;

use std::rc::Rc;

use speedyspec_data_query::PassChain;
use speedyspec_data_translate::{RowLimitPass, neutral_chain};

use crate::schema::SchemaCache;

pub(crate) use identifiers::PgIdentifiers;
pub(crate) use placeholders::PgPlaceholders;

/// The neutral passes, then PostgreSQL's, in order.
pub(crate) fn pg_chain(schema: &Rc<SchemaCache>) -> PassChain {
    let mut chain = neutral_chain();
    chain.push(Box::new(RowLimitPass::new("ctid")));
    chain.push(Box::new(PgIdentifiers::new(schema.clone())));
    chain.push(Box::new(PgPlaceholders));
    chain
}
