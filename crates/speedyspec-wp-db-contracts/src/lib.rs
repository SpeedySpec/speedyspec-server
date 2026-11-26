//! The database seam's contracts: the `wpdb` port other seams call, its values and its errors.
//!
//! - [`Wpdb`] is `wpdb` as other seams use it. `speedyspec-wp-db` implements it.
//! - [`QueryOutcome`], [`WpRow`], [`Field`], [`ColLength`] and [`LengthKind`] are the values it hands out.
//! - [`WpDbError`] is every failure of a `wpdb` method; [`take_hook_error`] hands back a hook callback's failure
//!   once, for a language bridge to rethrow.

mod api;
mod error;
mod values;

pub use api::Wpdb;
pub use error::{WpDbError, take_hook_error};
pub use values::{ColLength, Field, LengthKind, QueryOutcome, WpRow, WpValue};
