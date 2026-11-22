//! WordPress's theme.json engine (`WP_Theme_JSON`, `WP_Theme_JSON_Schema`) in Rust, split into domain pieces.
//!
//! `ThemeJsonApi` is the Facade behind the PHP class; each piece (migration, sanitizing, merging, presets, …) is a
//! module with one job.

mod codec;
mod php_eq;

pub use codec::ThemeJsonCodec;
pub use php_eq::php_loose_eq;
pub mod array_path;
mod schema_migrate;

pub use schema_migrate::{MigrationStep, migrate};
