//! WordPress's options API in Rust: the option entity, its values, the ports to code not yet ported, and the API.
//!
//! The entity is named `Option`, as the plan index fixes. Never glob-import this crate: write
//! `use speedyspec_wp_options::Option as OptionEntity;`, so `Option<T>` stays std's everywhere.

mod adapters;
mod api;
mod autoload;
mod autoload_policy;
mod autoload_set;
#[cfg(feature = "conformance")]
pub mod conformance;
mod entity;
mod error;
mod external_cache;
mod facade;
mod fired;
mod messages;
mod name;
mod ports;
mod read;
mod sitemeta;
mod staged;
mod value;
mod write;

pub use adapters::{
    CORE_SANITIZED_OPTIONS, FixedProtectedOptions, HookSanitizing, InstallingFlag, LogDeprecations,
    NoOptionGroups, RustCodec, SiteConfiguration,
};
pub use api::{OptionsApi, OptionsDeps};
pub use autoload::{Autoload, AutoloadRequest, DEFAULT_AUTOLOAD_VALUES};
pub use autoload_policy::{
    AutoloadPolicy, WordPressAutoloadPolicy, autoload_values,
    filter_default_autoload_value_via_option_size, register_default_filters,
};
pub use entity::Option;
pub use error::OptionsError;
pub use external_cache::{ExternalObjectCache, ObjectCachePersistence};
pub use facade::{FromOptionValue, Options};
pub use messages::{
    AddOption, AutoloadValuesToAutoload, DeleteOption, DetermineOptionAutoloadValue,
    FilterDefaultAutoloadValueViaOptionSize, GetOption, GetOptions, LoadAlloptions,
    PrimeOptionCaches, PrimeOptionCachesByGroup, ProtectSpecialOption, SetOptionAutoload,
    SetOptionAutoloadValues, SetOptionsAutoload, UpdateOption, register_options_handlers,
};
pub use name::OptionName;
pub use ports::{
    Deprecations, Installing, Multisite, OptionGroups, OptionRepository, ProtectedOptions,
    Sanitizing, ValueCodec,
};
pub use sitemeta::{NetworkMeta, SitemetaRepository};
pub use staged::Staged;
pub use value::{SerializedValue, php_empty, php_string};
