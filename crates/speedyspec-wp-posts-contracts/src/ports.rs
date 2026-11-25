//! The ports the post type and status rules read through; adapters implement them at the edge.

use speedyspec_php_serialize::PhpValue;

use crate::{PostStatusObject, PostType};

/// `__()` and `_x()` in the `default` domain (WordPress's English when nothing translates).
pub trait Translator {
    /// The translation of `text`, in `context` when there is one.
    fn translate(&self, text: &str, context: Option<&str>) -> String;
}

/// `set_props()`' `is_admin() || get_option( 'permalink_structure' )`.
pub trait RewriteContext {
    /// Whether rewrite arguments are resolved.
    fn rewrite_applies(&self) -> bool;
}

/// `sanitize_title_with_dashes()` for a string `query_var` (the text seam's rule; a PHP adapter in phase 4).
pub trait TitleSanitizer {
    /// The sanitized title.
    fn sanitize_title_with_dashes(&self, title: &[u8]) -> Vec<u8>;
}

/// `$wp_post_types`, read live by phase 4's PHP adapter. Owned values: the adapter converts PHP objects anyway.
pub trait PostTypes {
    /// One registered post type.
    fn post_type(&self, name: &str) -> Option<PostType>;
    /// Every registered post type, in registration order.
    fn post_types(&self) -> Vec<PostType>;
}

/// `$_wp_post_type_features`: post type, then feature, then `true` or the args array, in insertion order.
pub trait PostTypeSupports {
    /// `post_type_supports()`.
    fn supports(&self, post_type: &str, feature: &str) -> bool;
    /// Every post type's features.
    fn features(&self) -> Vec<(String, Vec<(String, PhpValue)>)>;
}

/// `$wp_post_statuses`.
///
/// Lookups take UTF-8 names. PHP keys statuses by bytes, so the adapter must decide how a non-UTF-8 name maps (no
/// stored status has one when every name went through `sanitize_key()`, which keeps only ASCII).
pub trait PostStatuses {
    /// One registered post status.
    fn status(&self, name: &str) -> Option<PostStatusObject>;
    /// Every registered post status, in registration order.
    fn statuses(&self) -> Vec<PostStatusObject>;
}
