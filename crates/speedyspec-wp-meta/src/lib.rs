//! WordPress's metadata API rules: the protected-key rule and the meta hook names.

mod hook_names;
mod protected;

pub use hook_names::MetaHookNames;
pub use protected::is_protected_key;
