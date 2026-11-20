//! The PostgreSQL passes, run after the neutral ones (Chain of Responsibility, each a Visitor over the tree).

mod collation;
mod ddl;
mod expressions;
mod functions;
mod identifiers;
mod placeholders;
mod zero_dates;

use std::rc::Rc;

use speedyspec_data_query::PassChain;
use speedyspec_data_translate::{RowLimitPass, neutral_chain};

use crate::schema::SchemaCache;

pub(crate) use collation::PgCollation;
pub(crate) use ddl::PgDdl;
pub(crate) use expressions::PgExpressions;
pub(crate) use identifiers::PgIdentifiers;
pub(crate) use placeholders::PgPlaceholders;
pub(crate) use zero_dates::PgZeroDates;

/// The neutral passes, then PostgreSQL's, in order.
pub(crate) fn pg_chain(schema: &Rc<SchemaCache>) -> PassChain {
    let mut chain = neutral_chain();
    chain.push(Box::new(RowLimitPass::new("ctid")));
    chain.push(Box::new(PgDdl::new(schema.clone())));
    chain.push(Box::new(PgExpressions::new(schema.clone())));
    chain.push(Box::new(PgCollation));
    chain.push(Box::new(PgZeroDates::new(schema.clone())));
    chain.push(Box::new(PgIdentifiers::new(schema.clone())));
    chain.push(Box::new(PgPlaceholders));
    chain
}
