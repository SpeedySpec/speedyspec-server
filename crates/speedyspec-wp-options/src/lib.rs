//! WordPress's options API in Rust: the option entity, its values, the ports to code not yet ported, and the API.
//!
//! The entity is named `Option`, as the plan index fixes. Never glob-import this crate: write
//! `use speedyspec_wp_options::Option as OptionEntity;`, so `Option<T>` stays std's everywhere.

mod adapters;
mod autoload;
mod entity;
mod error;
mod name;
mod ports;
mod value;

pub use adapters::{
    CORE_SANITIZED_OPTIONS, FixedProtectedOptions, HookSanitizing, InstallingFlag, LogDeprecations,
    NoOptionGroups, RustCodec, SiteConfiguration,
};
pub use autoload::{Autoload, AutoloadRequest, DEFAULT_AUTOLOAD_VALUES};
pub use entity::Option;
pub use error::OptionsError;
pub use name::OptionName;
pub use ports::{
    Deprecations, Installing, Multisite, OptionGroups, OptionRepository, ProtectedOptions,
    Sanitizing, ValueCodec,
};
pub use value::{SerializedValue, php_empty, php_string};
