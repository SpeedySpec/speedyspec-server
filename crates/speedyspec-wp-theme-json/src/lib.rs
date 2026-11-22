//! WordPress's theme.json engine (`WP_Theme_JSON`, `WP_Theme_JSON_Schema`) in Rust, split into domain pieces.
//!
//! `ThemeJsonApi` is the Facade behind the PHP class; each piece (migration, sanitizing, merging, presets, …) is a
//! module with one job.

pub(crate) mod codec;
mod php_eq;

pub use codec::ThemeJsonCodec;
pub use php_eq::php_loose_eq;
pub mod array_path;
pub mod php;
pub mod sanitize;
mod schema_migrate;

pub use schema_migrate::{MigrationStep, migrate};

/// A PHP string as the Hook API carries it (`Str` for UTF-8, `Bytes` otherwise).
pub fn codec_string(bytes: &[u8]) -> speedyspec_hooks::Value {
    codec::string_value(bytes)
}
mod construct;
pub mod css;
pub mod declarations;
mod deps;
pub mod filters;
mod memory;
pub mod nodes;
#[allow(dead_code)] // Used by the stylesheet pieces as they land.
pub(crate) mod php_value;
pub mod presets;
pub mod selectors;

pub use construct::{construct, construct_value, valid_names};
pub use deps::ThemeJsonDeps;
pub use memory::{MemoryBlockMetadata, MemoryGlobalSettings, NoTranslation, RecordedDiagnostics};
pub mod merge;

pub use merge::merge;
mod api;
mod data;
pub mod messages;

pub use api::ThemeJsonApi;
pub use messages::register_theme_json_handlers;
