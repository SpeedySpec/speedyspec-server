//! Style declarations: `compute_style_properties`, `get_property_value` (with `ref` following), the slice of the
//! style engine theme.json uses for background images, the feature-level declarations of a node, and the
//! separator, button width and paragraph text-indent adjustments.

mod background;
mod features;
mod properties;

pub use background::background_image_value;
pub use features::{
    FeatureDeclarations, feature_declarations_for_node, update_button_width_declarations,
    update_paragraph_text_indent_selector, update_separator_declarations,
};
pub use properties::{compute_style_properties, property_value};

use speedyspec_wp_theme_json_contracts::ports::{Diagnostics, Gettext};

use crate::presets::FluidFontSize;

/// What computing declarations needs from outside.
#[derive(Clone, Copy)]
pub struct DeclarationContext<'a> {
    /// Fluid font sizes for `font-size`.
    pub fluid: &'a FluidFontSize,
    /// Where `_doing_it_wrong` goes.
    pub diagnostics: &'a dyn Diagnostics,
    /// `__()` for WordPress's messages.
    pub gettext: &'a dyn Gettext,
}
