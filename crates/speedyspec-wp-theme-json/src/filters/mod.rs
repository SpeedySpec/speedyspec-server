//! The `ThemeJsonFilters` adapters: through the Rust Hook API, and the Null Object for no listeners.

use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_wp_hooks::{Count, HookApi, HookName};
use speedyspec_wp_theme_json_contracts::ThemeJsonError;
use speedyspec_wp_theme_json_contracts::ports::ThemeJsonFilters;

/// The filters through the Rust Hook API, so PHP plugins' callbacks keep working (an Adapter).
///
/// Each filter goes through [`HookApi::skip_if_empty`] first, as the shortcode and options ports do.
pub struct HookApiFilters {
    hooks: Rc<HookApi>,
}

impl HookApiFilters {
    /// Filters over `hooks`.
    pub fn new(hooks: Rc<HookApi>) -> HookApiFilters {
        HookApiFilters { hooks }
    }

    fn filter(&self, hook: &str, value: Value) -> Result<Value, ThemeJsonError> {
        let hook = HookName::from(hook);
        let error = |e: speedyspec_wp_hooks::HookError| ThemeJsonError::Callback(e.message);
        if self
            .hooks
            .skip_if_empty(&hook, Count::Filters)
            .map_err(error)?
        {
            return Ok(value);
        }
        let filtered = self
            .hooks
            .apply_filters(&hook, value, Vec::new())
            .map_err(error)?;
        Ok(filtered.read().into_owned())
    }
}

impl ThemeJsonFilters for HookApiFilters {
    fn style_nodes(&self, nodes: Value) -> Result<Value, ThemeJsonError> {
        self.filter("wp_theme_json_get_style_nodes", nodes)
    }
}

/// No filters (a Null Object): every value passes through unchanged.
pub struct NoFilters;

impl ThemeJsonFilters for NoFilters {
    fn style_nodes(&self, nodes: Value) -> Result<Value, ThemeJsonError> {
        Ok(nodes)
    }
}
