//! WordPress's metadata API rules: the protected-key rule, the meta hook names, `register_meta()`'s rules and the
//! registered-keys registry (`$wp_meta_keys`).

mod hook_names;
mod php;
mod protected;
mod register;
mod registry;

pub use hook_names::MetaHookNames;
pub use protected::is_protected_key;
pub use register::{
    FilterToAdd, RegisterArgs, RegisterPorts, RegisteredMetaKey, Registration, allowed_args,
    resolve_registration,
};
pub use registry::{MetaKeyRegistry, filters_to_remove};
