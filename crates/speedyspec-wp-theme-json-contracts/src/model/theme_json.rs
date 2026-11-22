use crate::model::{OrderedMap, PhpString, Settings, StyleNode};
use crate::{Key, Leaf, Value};

/// A theme.json document (`VALID_TOP_LEVEL_KEYS`).
///
/// `blockTypes`, `customTemplates`, `templateParts` and `patterns` are lists the engine hands back without reading
/// into, so they stay PHP values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThemeJson {
    /// `blockTypes`.
    pub block_types: Option<Value>,
    /// `customTemplates`.
    pub custom_templates: Option<Value>,
    /// `description`.
    pub description: Option<Leaf<PhpString>>,
    /// `patterns`.
    pub patterns: Option<Value>,
    /// `settings`.
    pub settings: Option<Leaf<Settings>>,
    /// `slug`.
    pub slug: Option<Leaf<PhpString>>,
    /// `styles`.
    pub styles: Option<Leaf<StyleNode>>,
    /// `templateParts`.
    pub template_parts: Option<Value>,
    /// `title`.
    pub title: Option<Leaf<PhpString>>,
    /// `version`.
    pub version: Option<Leaf<i64>>,
    /// Keys the schema does not name, in the data's order.
    pub extra: OrderedMap<Key, Value>,
}

impl ThemeJson {
    /// The schema's top-level keys, in the schema's order.
    pub const KEYS: &'static [&'static str] = &[
        "blockTypes",
        "customTemplates",
        "description",
        "patterns",
        "settings",
        "slug",
        "styles",
        "templateParts",
        "title",
        "version",
    ];

    /// `array( 'version' => WP_Theme_JSON::LATEST_SCHEMA )`, WordPress's default document.
    pub fn latest() -> ThemeJson {
        ThemeJson {
            version: Some(Leaf::Typed(crate::schema::LATEST_SCHEMA)),
            ..ThemeJson::default()
        }
    }
}
