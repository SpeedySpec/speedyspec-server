//! The names of the hooks `meta.php` fires, for one meta type.
//!
//! Names are copied from WordPress 7.1.5's `wp-includes/meta.php`. A key is bytes, so the names that hold one are
//! bytes too.

use speedyspec_wp_meta_contracts::{MetaKey, MetaType};

/// The hook names for one meta type (`post`, `user`, or a custom type).
#[derive(Clone, Debug)]
pub struct MetaHookNames {
    meta_type: String,
}

impl MetaHookNames {
    pub fn new(meta_type: &MetaType) -> MetaHookNames {
        MetaHookNames {
            meta_type: meta_type.name().to_owned(),
        }
    }

    fn named(&self, prefix: &str, suffix: &str) -> String {
        format!("{prefix}{}{suffix}", self.meta_type)
    }

    fn with_key(&self, prefix: &str, key: &MetaKey, subtype: Option<&str>) -> Vec<u8> {
        let mut name = format!("{prefix}{}_meta_", self.meta_type).into_bytes();
        name.extend_from_slice(key.as_bytes());
        if let Some(subtype) = subtype {
            name.extend_from_slice(b"_for_");
            name.extend_from_slice(subtype.as_bytes());
        }
        name
    }

    /// `sanitize_meta()`: `sanitize_{type}_meta_{key}`, or with `_for_{subtype}` when there is a subtype.
    pub fn sanitize(&self, key: &MetaKey, subtype: Option<&str>) -> Vec<u8> {
        self.with_key("sanitize_", key, subtype)
    }

    /// `register_meta()` and `unregister_meta_key()`: `auth_{type}_meta_{key}`, or with `_for_{subtype}`.
    pub fn auth(&self, key: &MetaKey, subtype: Option<&str>) -> Vec<u8> {
        self.with_key("auth_", key, subtype)
    }

    /// `add_metadata()`: the short-circuit filter `add_{type}_metadata`.
    pub fn add_pre(&self) -> String {
        self.named("add_", "_metadata")
    }

    /// `add_metadata()`: the action `add_{type}_meta`, before the insert.
    pub fn add(&self) -> String {
        self.named("add_", "_meta")
    }

    /// `add_metadata()`: the action `added_{type}_meta`, after the insert.
    pub fn added(&self) -> String {
        self.named("added_", "_meta")
    }

    /// `update_metadata()`: the short-circuit filter `update_{type}_metadata`.
    pub fn update_pre(&self) -> String {
        self.named("update_", "_metadata")
    }

    /// `update_metadata()` and `update_metadata_by_mid()`: the action `update_{type}_meta`, before the update.
    pub fn update(&self) -> String {
        self.named("update_", "_meta")
    }

    /// `update_metadata()` and `update_metadata_by_mid()`: the action `updated_{type}_meta`, after the update.
    pub fn updated(&self) -> String {
        self.named("updated_", "_meta")
    }

    /// `delete_metadata()`: the short-circuit filter `delete_{type}_metadata`.
    pub fn delete_pre(&self) -> String {
        self.named("delete_", "_metadata")
    }

    /// `delete_metadata()` and `delete_metadata_by_mid()`: the action `delete_{type}_meta`, before the delete.
    pub fn delete(&self) -> String {
        self.named("delete_", "_meta")
    }

    /// `delete_metadata()` and `delete_metadata_by_mid()`: the action `deleted_{type}_meta`, after the delete.
    pub fn deleted(&self) -> String {
        self.named("deleted_", "_meta")
    }

    /// `get_metadata_raw()` and `metadata_exists()`: the short-circuit filter `get_{type}_metadata`.
    pub fn get_pre(&self) -> String {
        self.named("get_", "_metadata")
    }

    /// `get_metadata_by_mid()`: the short-circuit filter `get_{type}_metadata_by_mid`.
    pub fn get_by_mid_pre(&self) -> String {
        self.named("get_", "_metadata_by_mid")
    }

    /// `update_metadata_by_mid()`: the short-circuit filter `update_{type}_metadata_by_mid`.
    pub fn update_by_mid_pre(&self) -> String {
        self.named("update_", "_metadata_by_mid")
    }

    /// `delete_metadata_by_mid()`: the short-circuit filter `delete_{type}_metadata_by_mid`.
    pub fn delete_by_mid_pre(&self) -> String {
        self.named("delete_", "_metadata_by_mid")
    }

    /// `get_metadata_default()`: the filter `default_{type}_metadata`.
    pub fn default(&self) -> String {
        self.named("default_", "_metadata")
    }

    /// `update_meta_cache()`: the short-circuit filter `update_{type}_metadata_cache`.
    pub fn cache_pre(&self) -> String {
        self.named("update_", "_metadata_cache")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(kind: &str) -> MetaHookNames {
        MetaHookNames::new(&MetaType::new(kind).unwrap())
    }

    #[test]
    fn post_names() {
        let n = names("post");
        assert_eq!(n.add_pre(), "add_post_metadata");
        assert_eq!(n.add(), "add_post_meta");
        assert_eq!(n.added(), "added_post_meta");
        assert_eq!(n.update_pre(), "update_post_metadata");
        assert_eq!(n.update(), "update_post_meta");
        assert_eq!(n.updated(), "updated_post_meta");
        assert_eq!(n.delete_pre(), "delete_post_metadata");
        assert_eq!(n.delete(), "delete_post_meta");
        assert_eq!(n.deleted(), "deleted_post_meta");
        assert_eq!(n.get_pre(), "get_post_metadata");
        assert_eq!(n.get_by_mid_pre(), "get_post_metadata_by_mid");
        assert_eq!(n.update_by_mid_pre(), "update_post_metadata_by_mid");
        assert_eq!(n.delete_by_mid_pre(), "delete_post_metadata_by_mid");
        assert_eq!(n.default(), "default_post_metadata");
        assert_eq!(n.cache_pre(), "update_post_metadata_cache");
    }

    #[test]
    fn user_names() {
        let n = names("user");
        assert_eq!(n.add_pre(), "add_user_metadata");
        assert_eq!(n.updated(), "updated_user_meta");
        assert_eq!(n.delete_by_mid_pre(), "delete_user_metadata_by_mid");
        assert_eq!(n.cache_pre(), "update_user_metadata_cache");
    }

    #[test]
    fn sanitize_and_auth_hold_the_key_and_subtype() {
        let n = names("post");
        let key = MetaKey::from("color");
        assert_eq!(n.sanitize(&key, None), b"sanitize_post_meta_color");
        assert_eq!(
            n.sanitize(&key, Some("book")),
            b"sanitize_post_meta_color_for_book"
        );
        assert_eq!(n.auth(&key, None), b"auth_post_meta_color");
        assert_eq!(n.auth(&key, Some("book")), b"auth_post_meta_color_for_book");
        let raw = MetaKey::new(vec![0xFF, b'k']);
        assert_eq!(n.sanitize(&raw, None), b"sanitize_post_meta_\xFFk");
    }
}
