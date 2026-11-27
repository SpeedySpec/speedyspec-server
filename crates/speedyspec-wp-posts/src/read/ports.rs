//! The outbound ports the read use cases call for code that is not ported yet (Adapter): users, taxonomy, the clock
//! `wp_cache_set_last_changed()` reads, and the `$_wp_suspend_cache_invalidation` global. Phase 4's PHP adapters
//! call WordPress's own functions; the Rust adapters come with the users and taxonomy ports.

use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_posts_contracts::{PostError, PostId};

/// The users functions the post reads call (user.php, capabilities.php, pluggable.php). Each method stands for the
/// WordPress function it is named after, its own hooks included; a callback's failure is [`PostError::Hook`].
pub trait Users {
    /// `is_user_logged_in()` (pluggable).
    fn is_user_logged_in(&self) -> Result<bool, PostError>;
    /// `get_current_user_id()`: 0 when nobody is logged in.
    fn get_current_user_id(&self) -> u64;
    /// `current_user_can( $capability )`, with the capability as PHP holds it (a post type's `cap` property may hold
    /// anything a plugin put there).
    fn current_user_can(&self, capability: &PhpValue) -> Result<bool, PostError>;
}

/// The taxonomy functions the post caches call (taxonomy.php, class-wp-post.php). Each method stands for the
/// WordPress code its doc names, its own hooks and cache writes included.
pub trait Taxonomy {
    /// `update_object_term_cache( $object_ids, $object_type )`: `object_types` are the values of the array
    /// `_prime_post_caches()` builds with `array_unique( array_map( 'get_post_type', $ids ) )`, so `false` for an id
    /// with no post.
    fn update_object_term_cache(
        &self,
        ids: &[PostId],
        object_types: &[PhpValue],
    ) -> Result<(), PostError>;
    /// `clean_object_term_cache( $object_ids, $object_type )`.
    fn clean_object_term_cache(&self, ids: &[PostId], object_type: &str) -> Result<(), PostError>;
    /// `WP_Post::__get()`'s `post_category` and `tags_input` branch for `post` (a `WP_Post`): when
    /// `is_object_in_taxonomy( $post->post_type, $taxonomy )`, `wp_list_pluck( get_the_terms( $post, $taxonomy ),
    /// $field )`; an empty list or a `WP_Error` gives `array()`. `post_category` is `( 'category', 'term_id' )` and
    /// `tags_input` `( 'post_tag', 'name' )`.
    fn post_term_fields(
        &self,
        post: &PhpValue,
        taxonomy: &str,
        field: &str,
    ) -> Result<PhpValue, PostError>;
}

/// PHP's `microtime()` with no argument: `"0.12345600 1700000000"`, the value `wp_cache_set_last_changed()` stores.
pub trait Microtime {
    /// The current time as `microtime()` prints it.
    fn microtime(&self) -> String;
}

/// `$_wp_suspend_cache_invalidation`, which `wp_suspend_cache_invalidation()` sets.
pub trait CacheInvalidation {
    /// `! empty( $_wp_suspend_cache_invalidation )`.
    fn suspended(&self) -> bool;
}
