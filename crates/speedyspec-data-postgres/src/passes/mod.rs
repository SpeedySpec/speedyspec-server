//! The PostgreSQL passes, run after the neutral ones (Chain of Responsibility, each a Visitor over the tree).

mod collation;
mod ddl;
mod deletes;
mod expressions;
mod functions;
mod group_by;
mod identifiers;
mod params;
mod placeholders;
mod writes;
mod zero_dates;

use std::rc::Rc;

use speedyspec_data_query::PassChain;
use speedyspec_data_translate::{RowLimitPass, neutral_chain};

use crate::schema::SchemaCache;

pub(crate) use collation::PgCollation;
pub(crate) use ddl::PgDdl;
pub(crate) use deletes::PgMultiDelete;
pub(crate) use expressions::PgExpressions;
pub(crate) use group_by::PgLooseGroupBy;
pub(crate) use identifiers::PgIdentifiers;
pub(crate) use placeholders::PgPlaceholders;
pub(crate) use writes::{PgChangedRows, PgInsertId, PgReplace, PgUpsert};
pub(crate) use zero_dates::PgZeroDates;

/// The neutral passes, then PostgreSQL's, in order.
pub(crate) fn pg_chain(schema: &Rc<SchemaCache>) -> PassChain {
    let mut chain = neutral_chain();
    chain.push(Box::new(RowLimitPass::new("ctid")));
    chain.push(Box::new(PgDdl::new(schema.clone())));
    chain.push(Box::new(PgMultiDelete));
    chain.push(Box::new(PgReplace {
        schema: schema.clone(),
    }));
    chain.push(Box::new(PgUpsert {
        schema: schema.clone(),
    }));
    chain.push(Box::new(PgInsertId {
        schema: schema.clone(),
    }));
    chain.push(Box::new(PgChangedRows {
        schema: schema.clone(),
    }));
    chain.push(Box::new(PgLooseGroupBy {
        schema: schema.clone(),
    }));
    chain.push(Box::new(PgExpressions::new(schema.clone())));
    chain.push(Box::new(PgCollation {
        schema: schema.clone(),
    }));
    chain.push(Box::new(PgZeroDates::new(schema.clone())));
    chain.push(Box::new(PgIdentifiers::new(schema.clone())));
    chain.push(Box::new(PgPlaceholders));
    chain
}
