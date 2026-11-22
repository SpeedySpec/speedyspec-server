//! The ports: what the theme.json engine needs from outside, each implemented by an adapter elsewhere.

mod active_theme;
mod block_metadata;
mod capabilities;
mod diagnostics;
mod filters;
mod gettext;
mod global_settings;
mod resolver;
mod text;

pub use active_theme::ActiveTheme;
pub use block_metadata::{BlockMetadata, RegisteredBlock};
pub use capabilities::Capabilities;
pub use diagnostics::Diagnostics;
pub use filters::ThemeJsonFilters;
pub use gettext::Gettext;
pub use global_settings::GlobalSettings;
pub use resolver::{
    BlockStyleVariationRegistrar, FilteredLayer, GlobalStylesPost, LayerFilters, WpCache,
};
pub use text::TextFunctions;

/// kses's `safecss_filter_attr()`, which `is_safe_css_declaration` asks.
pub use speedyspec_wp_kses_contracts::SafeCss;
