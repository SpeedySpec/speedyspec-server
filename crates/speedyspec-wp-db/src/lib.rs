//! WordPress's `wpdb`, in Rust.
//!
//! [`WpDb`] is the port's API (Gateway) and the handler of every command and query. Pure domain services hold
//! WordPress's rules; `WpdbState` and `WpdbHost` are its outbound ports.

mod config;
mod error;

pub use config::{DbHost, WpDbConfig};
pub use error::WpDbError;

mod escape;
mod php;
mod prepare;

pub use escape::{PlaceholderEscape, esc_like, escape, quote_identifier, real_escape, weak_escape};
pub use php::float_to_string;
pub use prepare::{DoingItWrong, Prepare, PrepareOutcome};

/// A PHP value, as the Hook API carries it.
pub type WpValue = speedyspec_hooks::Value;

mod tables;

pub use tables::{TableEnv, TableNames, TableScope};

mod fields;
mod query_text;

pub use fields::{ColLength, Field, FieldFormats, LengthKind, delete_sql, insert_sql, update_sql};
pub use query_text::{QueryClass, get_table_from_query};

mod caps;
mod charset;

pub use caps::ServerCaps;
pub use charset::{CharsetRules, Convert, StripContext};

mod host;
mod shape;
mod state;

pub use host::{HostCall, MemoryWpdbHost, WpdbHost};
pub use shape::{LastQuery, OutputShape, Shaped, WpRow, var};
pub use state::{ColumnMeta, LoggedQuery, MemoryWpdbState, RequestState, WPDB_STATE, WpdbState};
