use crate::{ThemeJsonError, Value};

/// The filters `WP_Theme_JSON` applies (one method per filter; the data filters join in plan 4).
pub trait ThemeJsonFilters {
    /// `apply_filters( 'wp_theme_json_get_style_nodes', $nodes )`: the style nodes, as WordPress's list of node
    /// arrays, possibly removed from, reordered or changed by a callback.
    fn style_nodes(&self, nodes: Value) -> Result<Value, ThemeJsonError>;
}
