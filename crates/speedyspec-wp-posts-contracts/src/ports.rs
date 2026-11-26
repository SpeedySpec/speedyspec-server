//! The ports the post entity, post type and status rules read through; adapters implement them at the edge.
//!
//! Every [`PostRepository`] method is one `$wpdb` call `post.php` makes, so query counts match. A failure `$wpdb`
//! hides is `Ok` with the empty answer; `Err` is only for failures it would not hide. Rows are compared byte for
//! byte; collation is the SQL adapter's.

use speedyspec_php_serialize::PhpValue;

use crate::{Post, PostError, PostId, PostStatusObject, PostType, PostTypeName, Slug};

/// `wp_posts` (Repository). `{posts}` is `$wpdb->posts`.
pub trait PostRepository {
    /// The name of the engine behind this repository (Strategy per engine).
    fn engine(&self) -> String;
    /// `WP_Post::get_instance()`: `SELECT * FROM {posts} WHERE ID = %d LIMIT 1`.
    fn find(&self, id: PostId) -> Result<Option<Post>, PostError>;
    /// `wp_insert_post()`'s `$wpdb->insert( $wpdb->posts, $data )`: the 21 columns it writes, plus `ID` when
    /// `post.id` is non-zero (the `import_id` case); `comment_count` is not written (the column default, 0). The
    /// new id; `None` when `$wpdb->insert` returns false (for example a duplicate ID).
    ///
    /// `wp_insert_post()` first runs a separate `SELECT ID FROM {posts} WHERE ID = %d` (post.php 5251-5256,
    /// WordPress 7.2-RC2) and drops a taken `import_id`; that pre-check is a later phase's own repository method.
    fn insert(&self, post: &Post) -> Result<Option<PostId>, PostError>;
    /// `wp_unique_post_slug()`, attachments:
    /// `SELECT post_name FROM {posts} WHERE post_name = %s AND ID != %d LIMIT 1`.
    fn attachment_slug_owner(&self, slug: &Slug, except: PostId)
    -> Result<Option<Slug>, PostError>;
    /// `wp_unique_post_slug()`, hierarchical types: `SELECT post_name FROM {posts} WHERE post_name = %s AND
    /// post_type IN ( %s, 'attachment' ) AND ID != %d AND post_parent = %d LIMIT 1`.
    fn hierarchical_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
        parent: PostId,
    ) -> Result<Option<Slug>, PostError>;
    /// `wp_unique_post_slug()`, flat types:
    /// `SELECT post_name FROM {posts} WHERE post_name = %s AND post_type = %s AND ID != %d LIMIT 1`.
    fn flat_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
    ) -> Result<Option<Slug>, PostError>;
}

/// `get_post( $id )`: through whatever cache the binding keeps. Phase 2b's binding has none (`speedyspec-wp-posts`'
/// `Uncached`); phase 4's reads the `posts` group first.
pub trait PostReader {
    /// The post, or `None` when there is none.
    fn post(&self, id: PostId) -> Result<Option<Post>, PostError>;
}

/// The time functions WordPress's dates use (a table in tests; Rust and PHP adapters in phase 5).
pub trait Clock {
    /// `current_time( 'mysql' )`.
    fn now_local(&self) -> String;
    /// `current_time( 'mysql', true )`, which is also `gmdate( 'Y-m-d H:i:s' )`.
    fn now_gmt(&self) -> String;
    /// `get_date_from_gmt()`.
    fn date_from_gmt(&self, gmt: &[u8]) -> Vec<u8>;
    /// `get_gmt_from_date()`.
    fn gmt_from_date(&self, local: &[u8]) -> Vec<u8>;
    /// `strtotime()` in UTC; `None` is false.
    fn timestamp(&self, datetime: &[u8]) -> Option<i64>;
}

/// `wp_truncate_slug( $slug, $length )` (formatting.php, the text seam's); phase 4's provider binds
/// `speedyspec-wp-text`'s rule.
pub trait SlugTruncation {
    /// The slug, cut to `length` bytes as WordPress cuts it.
    fn truncate(&self, slug: &[u8], length: usize) -> Vec<u8>;
}

/// The `wp_unique_post_slug_is_bad_{attachment,hierarchical,flat}_slug` filters (Strategy), bound in phase 4.
pub trait BadSlugs {
    /// `wp_unique_post_slug_is_bad_attachment_slug`.
    fn bad_attachment_slug(&self, slug: &[u8]) -> bool;
    /// `wp_unique_post_slug_is_bad_hierarchical_slug`.
    fn bad_hierarchical_slug(&self, slug: &[u8], post_type: &str, parent: PostId) -> bool;
    /// `wp_unique_post_slug_is_bad_flat_slug`.
    fn bad_flat_slug(&self, slug: &[u8], post_type: &str) -> bool;
}

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
