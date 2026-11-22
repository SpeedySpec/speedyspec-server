use crate::{Origin, ThemeJsonError, Value};

/// What a `wp_theme_json_data_*` filter returned.
#[derive(Clone, Debug, PartialEq)]
pub enum FilteredLayer {
    /// A `WP_Theme_JSON_Data`: its document, already built (`get_theme_json()->get_raw_data()`).
    Built(Value),
    /// Another object: its `get_data()`, still to be built.
    Config(Value),
}

/// The data filters (`wp_theme_json_data_default`, `_blocks`, `_theme`, `_user`), which hand callbacks a
/// `WP_Theme_JSON_Data`.
pub trait LayerFilters {
    /// `apply_filters( $hook, new WP_Theme_JSON_Data( $config, $origin ) )`.
    fn apply(
        &self,
        hook: &str,
        origin: Origin,
        config: Value,
    ) -> Result<FilteredLayer, ThemeJsonError>;
}

/// The user's global styles post (`wp_global_styles`), found or created as the resolver does.
pub trait GlobalStylesPost {
    /// `get_user_data_from_wp_global_styles( $theme, $create_post, $post_status_filter )`: the post's fields, or an
    /// empty array.
    fn find(&self, stylesheet: &str, create: bool, statuses: &Value) -> Value;
}

/// `wp_register_block_style_variations_from_theme_json_partials( $variations )`.
pub trait BlockStyleVariationRegistrar {
    /// Registers the block style variations the theme's partials define.
    fn register(&self, variations: &Value);
}

/// WordPress's object cache (`wp_cache_get`, `wp_cache_set`, `wp_cache_delete`), which the global styles functions
/// call and WordPress's tests observe.
pub trait WpCache {
    /// `wp_cache_get( $key, $group )`: `None` for WordPress's `false`.
    fn get(&self, key: &str, group: &str) -> Option<Value>;
    /// `wp_cache_set( $key, $value, $group )`.
    fn set(&self, key: &str, value: Value, group: &str);
    /// `wp_cache_delete( $key, $group )`.
    fn delete(&self, key: &str, group: &str);
}
