//! WordPress's `wpdb`, in Rust.
//!
//! [`NativeWpdb`] is the port's API (Gateway) and the handler of every command and query. Pure domain services hold
//! WordPress's rules; `WpdbState` and `WpdbHost` are its outbound ports.

mod config;

pub use config::{DbHost, WpDbConfig};

mod escape;
mod php;
mod prepare;

pub use escape::{PlaceholderEscape, esc_like, escape, quote_identifier, real_escape, weak_escape};
pub use php::float_to_string;
pub use prepare::{DoingItWrong, Prepare, PrepareOutcome};

mod tables;

pub use tables::{TableEnv, TableNames, TableScope};

mod fields;
mod query_text;

pub use fields::{FieldFormats, delete_sql, insert_sql, update_sql};
pub use query_text::{QueryClass, get_table_from_query};

mod caps;
mod charset;

pub use caps::ServerCaps;
pub use charset::{CharsetRules, Convert, StripContext};

mod host;
mod shape;
mod state;

pub use host::{HostCall, MemoryWpdbHost, WpdbHost};
pub use shape::{LastQuery, OutputShape, Shaped, keyed_rows, var};
pub use state::{ColumnMeta, LoggedQuery, MemoryWpdbState, RequestState, WPDB_STATE, WpdbState};

mod api;
mod binary;
mod connection;
mod contract;
mod helpers;
mod log;
mod lookups;
mod timed;

pub use api::{NativeWpdb, WpDbParts};
pub use timed::{TimedSqlStore, Timing, microtime};

mod messages;

pub use messages::*;

mod properties;

pub use properties::{HIDDEN_PROPERTIES, TABLE_PROPERTIES};
