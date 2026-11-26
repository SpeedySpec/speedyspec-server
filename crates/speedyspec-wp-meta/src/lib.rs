//! WordPress's metadata API: the use cases of `meta.php` (`add_metadata()` and the functions beside it) over their
//! ports, plus its pure rules: protected keys, the meta hook names, `register_meta()`'s rules and the registered-keys
//! registry (`$wp_meta_keys`).
//!
//! Patterns: Template Method (one use case per function, generic over the [`MetaType`](speedyspec_wp_meta_contracts::MetaType)
//! Strategy and its [`MetaHookNames`]), Observer (the hook API), Repository per meta type (the
//! [`MetaRepositories`](speedyspec_wp_meta_contracts::MetaRepositories) port), Registry ([`MetaKeyRegistry`]),
//! Command/CQRS ([`register_meta_handlers`]) and Decorator ([`FilteredProtection`]).

mod api;
mod by_mid;
mod cache;
mod hook_names;
mod messages;
mod protected;
mod read;
mod register;
mod registry;
mod sanitize;
mod subtype;
mod unslash;
mod write;

pub use api::{MetaDeps, MetadataApi};
pub use by_mid::ByMidOutcome;
pub use hook_names::MetaHookNames;
pub use messages::*;
pub use protected::{UnfilteredProtection, is_protected_key};
pub use read::read_hooks;
pub use register::{
    FilterToAdd, PreparedArgs, RegisterPorts, Registration, allowed_args, prepare_registration,
    resolve_registration,
};
pub use registry::{MetaKeyIndex, MetaKeyRegistry, default_for, filters_to_remove};
pub use sanitize::{FilteredProtection, sanitize_meta};
pub use subtype::{BlogSubtypes, Sites, SubtypesByType, object_subtype};
pub use unslash::{stripslashes, wp_unslash};
pub use write::AddOutcome;
