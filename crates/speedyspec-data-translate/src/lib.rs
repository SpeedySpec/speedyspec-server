//! MySQL-to-portable rewrite passes shared by the SQLite and PostgreSQL engines, and the MySQL knowledge they need.
//!
//! Every pass is a Visitor over `sqlparser`'s tree, run as a link in a `PassChain` (Chain of Responsibility). Params
//! are numbered first ([`number_placeholders`]), so a pass may move or repeat expressions without touching them.

mod admin;
mod binary_prefix;
mod catalogue;
mod ddl;
mod emulate;
mod expressions;
mod found_rows;
mod hints;
mod insert;
mod lexical;
mod like;
mod mysql_type;
mod names;
mod params;
mod row_limit;
mod session;
#[cfg(feature = "testing")]
pub mod testing;

use speedyspec_data_query::PassChain;

pub use admin::AdminPass;
pub use binary_prefix::BinaryPrefixPass;
pub use catalogue::{
    CatalogueColumn, CatalogueIndex, CatalogueTable, ColumnPosition, IndexKind, SchemaChange,
    catalogue_ddl, catalogue_table, catalogue_tables, catalogue_writes, column_key, column_of,
    index_object_name, is_catalogue_table, read_catalogue,
};
pub use ddl::DdlPass;
pub use emulate::{answer_emulated, family_of, render_create_table};
pub use expressions::{ExpressionPass, function_call};
pub use found_rows::take_found_rows;
pub use hints::HintsPass;
pub use insert::InsertPass;
pub use lexical::LexicalPass;
pub use like::mysql_like;
pub use mysql_type::{MySqlType, Temporal, TypeFamily};
pub use names::{object_name, sql_ident, sql_literal};
pub use params::number_placeholders;
pub use row_limit::RowLimitPass;
pub use session::SessionVars;

/// The neutral passes, in order: administrative statements, DDL, `BINARY`'s precedence, lexical, hints,
/// expressions, inserts.
pub fn neutral_chain() -> PassChain {
    let mut chain = PassChain::new();
    chain.push(Box::new(AdminPass));
    chain.push(Box::new(DdlPass));
    chain.push(Box::new(BinaryPrefixPass));
    chain.push(Box::new(LexicalPass));
    chain.push(Box::new(HintsPass));
    chain.push(Box::new(ExpressionPass));
    chain.push(Box::new(InsertPass));
    chain
}
