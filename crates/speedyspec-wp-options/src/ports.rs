//! The options API's ports: the repository (one method per `$wpdb` call `option.php` makes, so query counts
//! match), the value codec, and the ports to WordPress code not yet ported.

use speedyspec_wp_hooks::HookValue;

use crate::Option as OptionEntity;
use crate::{Autoload, OptionName, OptionsError, SerializedValue};

/// `wp_options`, by name (Repository). A failure `wpdb` hides (it returns false or null and records `last_error`)
/// is `Ok` with the empty answer; `Err` is only for failures `wpdb` would not hide.
pub trait OptionRepository {
    /// The engine's name, for logs.
    fn engine(&self) -> String;

    /// `SELECT option_value ... WHERE option_name = %s LIMIT 1`; `quiet` suppresses `wpdb`'s errors, as the
    /// installing path does.
    fn value(
        &self,
        name: &OptionName,
        quiet: bool,
    ) -> Result<Option<SerializedValue>, OptionsError>;

    /// `SELECT autoload ... WHERE option_name = %s`.
    fn autoload_of(&self, name: &OptionName) -> Result<Option<Autoload>, OptionsError>;

    /// `SELECT option_name, option_value ... WHERE option_name IN (..)`.
    fn values_of(&self, names: &[String]) -> Result<Vec<(String, SerializedValue)>, OptionsError>;

    /// Every option whose autoload is one of `values`, quietly, in table order.
    fn autoloaded(&self, values: &[String])
    -> Result<Vec<(String, SerializedValue)>, OptionsError>;

    /// Every option, quietly, in table order (the installing path's fallback).
    fn all(&self) -> Result<Vec<(String, SerializedValue)>, OptionsError>;

    /// The names in each group whose stored autoload differs from the group's value.
    fn needing_autoload_change(
        &self,
        groups: &[(Autoload, Vec<String>)],
    ) -> Result<Vec<String>, OptionsError>;

    /// `UPDATE ... SET autoload = %s WHERE option_name IN (..)`; the affected-row count.
    fn set_autoload(&self, names: &[String], autoload: &Autoload) -> Result<u64, OptionsError>;

    /// `INSERT ... ON DUPLICATE KEY UPDATE`: MySQL's affected-row count (1 new, 2 changed, 0 the same).
    fn add(&self, option: &OptionEntity) -> Result<u64, OptionsError>;

    /// `wpdb::update` of the value and, when given, the autoload. `value` `None` writes SQL `NULL`.
    fn update(
        &self,
        name: &OptionName,
        value: Option<&SerializedValue>,
        autoload: Option<&Autoload>,
    ) -> Result<u64, OptionsError>;

    /// `wpdb::delete` by name.
    fn remove(&self, name: &OptionName) -> Result<u64, OptionsError>;
}

/// `maybe_serialize()`, `maybe_unserialize()` and the object copy `get_option` makes (Strategy: Rust values, or
/// PHP objects through PHP).
pub trait ValueCodec {
    /// Non-array scalars come back unchanged, as WordPress's does.
    fn maybe_serialize(&self, value: &HookValue) -> Result<HookValue, OptionsError>;
    fn maybe_unserialize(&self, value: HookValue) -> Result<HookValue, OptionsError>;
    fn clone_value(&self, value: HookValue) -> HookValue;
}

/// `wp_installing()` and `defined('WP_SETUP_CONFIG')`.
pub trait Installing {
    fn installing(&self) -> bool;
    fn setup_config(&self) -> bool;
}

/// `is_multisite()`, `get_current_blog_id()`, `get_current_network_id()`, `is_main_site()` and `is_main_network()`.
pub trait Multisite {
    fn is_multisite(&self) -> bool;
    fn blog_id(&self) -> u64;
    fn network_id(&self) -> u64;
    /// `is_main_site()`.
    fn is_main_site(&self) -> bool;
    /// `is_main_network()`.
    fn is_main_network(&self) -> bool;
}

/// `sanitize_option()`.
pub trait Sanitizing {
    fn sanitize_option(
        &self,
        name: &OptionName,
        value: HookValue,
    ) -> Result<HookValue, OptionsError>;
}

/// `_deprecated_argument()`. No phase-2 function calls `_doing_it_wrong()`, so it waits until one does.
pub trait Deprecations {
    fn deprecated_argument(
        &self,
        function: &str,
        version: &str,
        message: &str,
    ) -> Result<(), OptionsError>;
}

/// `wp_protect_special_option()`.
pub trait ProtectedOptions {
    fn protect(&self, name: &str) -> Result<(), OptionsError>;
}

/// The `$new_allowed_options` global `wp_prime_option_caches_by_group()` reads; it is filled by
/// `register_setting()` (phase 4), so it is a port until then.
pub trait OptionGroups {
    fn options_in(&self, group: &str) -> Option<Vec<String>>;
}
