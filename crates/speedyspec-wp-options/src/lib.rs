//! WordPress's options API in Rust: the option entity, its values, the ports to code not yet ported, and the API.
//!
//! The entity is named `Option`, as the plan index fixes. Never glob-import this crate: write
//! `use speedyspec_wp_options::Option as OptionEntity;`, so `Option<T>` stays std's everywhere.

mod adapters;
mod api;
mod autoload_policy;
mod autoload_set;
mod clock;
mod conditional;
#[cfg(feature = "conformance")]
pub mod conformance;
mod contract;
mod entity;
mod external_cache;
mod fired;
mod messages;
mod name;
mod network;
mod network_messages;
mod php_cast;
mod ports;
mod read;
mod settings;
mod settings_messages;
mod sitemeta;
mod staged;
mod transient_messages;
mod transients;
mod value;
mod write;

// The option API other seams call, its errors, the typed reader and the PHP casts, from the options contracts. They are
// re-exported so this crate's callers name them as before; `OptionsApi` implements `Options` and `ConditionalOptions`.
pub use speedyspec_wp_options_contracts::{
    Autoload, AutoloadRequest, ConditionalOptions, ConditionalWrite, DEFAULT_AUTOLOAD_VALUES,
    FromOptionValue, Options, OptionsError, TypedOptions, php_empty, php_string,
};

pub use adapters::{
    CORE_SANITIZED_OPTIONS, FixedProtectedOptions, HookSanitizing, InstallingFlag, LogDeprecations,
    NoOptionGroups, RustCodec, SiteConfiguration,
};
pub use api::{OptionsApi, OptionsDeps};
pub use autoload_policy::{
    AutoloadPolicy, WordPressAutoloadPolicy, autoload_values,
    filter_default_autoload_value_via_option_size, register_default_filters,
};
pub use clock::{Clock, SystemClock};
pub use entity::Option;
pub use external_cache::{ExternalObjectCache, ObjectCachePersistence};
pub use messages::{
    AddOption, AutoloadValuesToAutoload, DeleteOption, DetermineOptionAutoloadValue,
    FilterDefaultAutoloadValueViaOptionSize, GetOption, GetOptions, LoadAlloptions,
    PrimeOptionCaches, PrimeOptionCachesByGroup, ProtectSpecialOption, SetOptionAutoload,
    SetOptionAutoloadValues, SetOptionsAutoload, UpdateOption, register_options_handlers,
};
pub use name::OptionName;
pub use network::{NetworkOptionsApi, NetworkPorts, resolve_network_id};
pub use network_messages::{
    AddNetworkOption, DeleteNetworkOption, GetNetworkOption, LoadCoreSiteOptions,
    PrimeNetworkOptionCaches, UpdateNetworkOption, register_network_option_handlers,
};
pub use php_cast::php_int;
pub use ports::{
    Deprecations, Installing, Multisite, OptionGroups, OptionRepository, ProtectedOptions,
    Sanitizing, ValueCodec,
};
pub use settings::{
    ArgsParsing, FILTER_DEFAULT_OPTION, NoArgsParsing, SettingArgsInput, SettingsRegistry,
    SettingsStore,
};
pub use settings_messages::{
    FilterDefaultOption, GetRegisteredSettings, RegisterSetting, UnregisterSetting,
    register_settings_handlers,
};
pub use sitemeta::{NetworkMeta, SitemetaRepository};
pub use staged::Staged;
pub use transient_messages::{
    DeleteExpiredTransients, DeleteSiteTransient, DeleteTransient, GetSiteTransient, GetTransient,
    SetSiteTransient, SetTransient, register_transient_handlers,
};
pub use transients::{ExpiredTransients, TransientPorts, TransientPrefix, TransientsApi};
pub use value::SerializedValue;
