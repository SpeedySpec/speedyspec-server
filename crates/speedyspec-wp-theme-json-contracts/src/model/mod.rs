//! The typed theme.json model: plain structs with no behaviour beyond construction and access.
//!
//! Every struct keeps the keys it does not know in `extra`, and every leaf is a [`Leaf`], so any PHP array that
//! WordPress would keep survives a round trip. Struct fields follow the schema's key order; maps keyed by data
//! (blocks, elements, variations, states) keep the data's order.

mod preset;
mod settings;
mod styles;
mod theme_json;
mod values;

pub use crate::Leaf;
pub use preset::{Preset, Presets, Scales};
pub use settings::{
    BackgroundSettings, BlockVisibilitySettings, BorderSettings, ColorSettings, DimensionsSettings, LayoutSettings,
    LightboxSettings, PositionSettings, Settings, ShadowSettings, SpacingSettings, TypographySettings,
    ViewportSettings,
};
pub use styles::{
    BackgroundStyles, BorderStyles, ColorStyles, DimensionsStyles, FilterStyles, OutlineStyles, SpacingStyles,
    StyleNode, StyleProperties, TypographyStyles,
};
pub use theme_json::ThemeJson;
pub use values::{CssValue, OrderedMap, PhpString, StyleValue};

/// Declares a struct whose fields are the keys of one schema object, in the schema's order.
///
/// Each field is optional (absent in the data), the struct keeps unknown keys in `extra`, and `KEYS` lists the
/// schema keys in order for the codec.
macro_rules! schema_object {
    ($(#[$meta:meta])* $name:ident { $( $(#[$fmeta:meta])* $field:ident : $ty:ty = $key:literal ),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq)]
        pub struct $name {
            $( $(#[$fmeta])* pub $field: Option<$ty>, )*
            /// Keys the schema does not name, in the data's order.
            pub extra: $crate::model::OrderedMap<$crate::Key, $crate::Value>,
        }

        impl $name {
            /// The schema's keys, in the schema's order.
            pub const KEYS: &'static [&'static str] = &[$($key),*];
        }
    };
}
pub(crate) use schema_object;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_json_is_version_3_only() {
        let doc = ThemeJson::latest();
        assert_eq!(doc.version, Some(Leaf::Typed(3)));
        assert!(doc.settings.is_none());
        assert!(doc.styles.is_none());
        assert!(doc.custom_templates.is_none() && doc.template_parts.is_none() && doc.patterns.is_none());
        assert!(doc.title.is_none() && doc.slug.is_none() && doc.description.is_none() && doc.block_types.is_none());
        assert!(doc.extra.is_empty());
    }

    #[test]
    fn settings_sections_follow_valid_settings_keys() {
        assert_eq!(ColorSettings::KEYS.len(), 15);
        assert_eq!(ColorSettings::KEYS[0], "background");
        assert_eq!(TypographySettings::KEYS.len(), 16);
        assert_eq!(StyleProperties::KEYS, ["background", "border", "color", "dimensions", "filter", "outline", "shadow", "spacing", "typography", "css"]);
    }
}
