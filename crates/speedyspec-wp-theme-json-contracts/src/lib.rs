//! WordPress's theme.json contracts: the typed model, the value objects, the schema tables and the ports.
//!
//! Nothing here knows PHP, files or a database. The core crate (`speedyspec-wp-theme-json`) holds the domain
//! services; the PHP crate (`speedyspec-wp-theme-json-php`) adapts them to WordPress's classes.

mod by_origin;
pub mod schema;
mod error;
mod leaf;
mod origin;

pub use by_origin::ByOrigin;
pub use error::ThemeJsonError;
pub use leaf::Leaf;
pub use origin::Origin;

/// A PHP-shaped value, as the Hook API carries it.
pub type Value = speedyspec_hooks::Value;
/// A PHP array key.
pub type Key = speedyspec_hooks::Key;
