//! The ports' Rust adapters, for callers with no PHP.

use std::cell::Cell;
use std::rc::Rc;

use speedyspec_php_serialize::{
    from_hook_value, maybe_serialize, maybe_unserialize, to_hook_value,
};
use speedyspec_wp_hooks::{Count, HookApi, HookName, HookValue};

use crate::{
    Deprecations, Installing, Multisite, OptionGroups, OptionName, OptionsError, ProtectedOptions,
    Sanitizing, ValueCodec,
};

/// The names `sanitize_option()`'s `switch` handles, from WordPress's `formatting.php`. Their rules need PHP.
pub const CORE_SANITIZED_OPTIONS: &[&str] = &[
    "admin_email",
    "new_admin_email",
    "thumbnail_size_w",
    "thumbnail_size_h",
    "medium_size_w",
    "medium_size_h",
    "medium_large_size_w",
    "medium_large_size_h",
    "large_size_w",
    "large_size_h",
    "mailserver_port",
    "comment_max_links",
    "page_on_front",
    "page_for_posts",
    "rss_excerpt_length",
    "default_category",
    "default_email_category",
    "default_link_category",
    "close_comments_days_old",
    "comments_per_page",
    "thread_comments_depth",
    "users_can_register",
    "start_of_week",
    "site_icon",
    "fileupload_maxk",
    "posts_per_page",
    "posts_per_rss",
    "default_ping_status",
    "default_comment_status",
    "blogdescription",
    "blogname",
    "blog_charset",
    "blog_public",
    "date_format",
    "time_format",
    "mailserver_url",
    "mailserver_login",
    "mailserver_pass",
    "upload_path",
    "ping_sites",
    "gmt_offset",
    "siteurl",
    "home",
    "show_on_front",
    "illegal_names",
    "limited_email_domains",
    "banned_email_domains",
    "timezone_string",
    "permalink_structure",
    "category_base",
    "tag_base",
    "default_role",
    "moderation_keys",
    "disallowed_keys",
    "WPLANG",
];

fn convert(e: impl std::fmt::Display) -> OptionsError {
    OptionsError::Serialize(e.to_string())
}

/// `maybe_serialize()` and `maybe_unserialize()` in Rust (`speedyspec-php-serialize`); objects stay opaque.
pub struct RustCodec;

impl ValueCodec for RustCodec {
    fn maybe_serialize(&self, value: &HookValue) -> Result<HookValue, OptionsError> {
        Ok(to_hook_value(&maybe_serialize(
            &from_hook_value(value).map_err(convert)?,
        )))
    }

    fn maybe_unserialize(&self, value: HookValue) -> Result<HookValue, OptionsError> {
        Ok(to_hook_value(&maybe_unserialize(
            &from_hook_value(&value).map_err(convert)?,
        )))
    }

    /// Rust values are copied by value already; an opaque PHP object cannot be cloned here and is returned as is.
    fn clone_value(&self, value: HookValue) -> HookValue {
        value
    }
}

/// `wp_installing()` as a flag the host sets.
#[derive(Default)]
pub struct InstallingFlag(Cell<bool>);

impl InstallingFlag {
    pub fn new() -> InstallingFlag {
        InstallingFlag(Cell::new(false))
    }

    pub fn set(&self, installing: bool) {
        self.0.set(installing);
    }
}

impl Installing for InstallingFlag {
    fn installing(&self) -> bool {
        self.0.get()
    }

    fn setup_config(&self) -> bool {
        false
    }
}

/// A site's place in the network, as configured.
#[derive(Clone, Copy, Debug)]
pub struct SiteConfiguration {
    pub multisite: bool,
    pub blog_id: u64,
    pub network_id: u64,
}

impl Multisite for SiteConfiguration {
    fn is_multisite(&self) -> bool {
        self.multisite
    }

    fn blog_id(&self) -> u64 {
        self.blog_id
    }

    fn network_id(&self) -> u64 {
        self.network_id
    }

    /// One network as configured: its main site is blog 1, as WordPress installs it.
    fn is_main_site(&self) -> bool {
        !self.multisite || self.blog_id == 1
    }

    /// A configuration names one network, so it is the main one.
    fn is_main_network(&self) -> bool {
        true
    }
}

/// `sanitize_option()` for options core does not sanitize itself: the `sanitize_option_{$option}` filter alone.
pub struct HookSanitizing {
    hooks: Rc<HookApi>,
}

impl HookSanitizing {
    pub fn new(hooks: Rc<HookApi>) -> HookSanitizing {
        HookSanitizing { hooks }
    }
}

impl Sanitizing for HookSanitizing {
    fn sanitize_option(
        &self,
        name: &OptionName,
        value: HookValue,
    ) -> Result<HookValue, OptionsError> {
        if CORE_SANITIZED_OPTIONS.contains(&name.as_str()) {
            return Err(OptionsError::NeedsPhp {
                function: "sanitize_option",
                option: name.as_str().to_string(),
            });
        }
        let hook = HookName::from(format!("sanitize_option_{name}").as_str());
        if self.hooks.skip_if_empty(&hook, Count::Filters)? {
            return Ok(value);
        }
        let original = value.clone();
        Ok(self.hooks.apply_filters(
            &hook,
            value,
            vec![HookValue::Str(name.as_str().to_string()), original],
        )?)
    }
}

/// `_deprecated_argument()` as a log line.
pub struct LogDeprecations;

impl Deprecations for LogDeprecations {
    fn deprecated_argument(
        &self,
        function: &str,
        version: &str,
        message: &str,
    ) -> Result<(), OptionsError> {
        tracing::warn!(function, version, message, "deprecated argument");
        Ok(())
    }
}

/// `wp_protect_special_option()`: `alloptions` and `notoptions` are the cache's, never options.
pub struct FixedProtectedOptions;

impl ProtectedOptions for FixedProtectedOptions {
    fn protect(&self, name: &str) -> Result<(), OptionsError> {
        if name == "alloptions" || name == "notoptions" {
            return Err(OptionsError::Protected(format!(
                "{name} is a protected WP option and may not be modified"
            )));
        }
        Ok(())
    }
}

/// No `register_setting()` groups (phase 4 fills them).
pub struct NoOptionGroups;

impl OptionGroups for NoOptionGroups {
    fn options_in(&self, _group: &str) -> Option<Vec<String>> {
        None
    }
}
