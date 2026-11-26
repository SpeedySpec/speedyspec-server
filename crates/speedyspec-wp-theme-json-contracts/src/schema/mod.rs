//! WordPress's `WP_Theme_JSON` class constants, from the release `data_release!` names: the schema tables every
//! piece of the engine reads.
//!
//! The values are WordPress's own, exported by `tools/export-constants.php` as PHP `serialize()` output and
//! embedded here, so they cannot drift from the release by a transcription mistake. The compat scenarios
//! `constant_*` check them against the release again through PHP.

use std::rc::Rc;

use crate::{Key, Value};

/// The newest theme.json schema version (`WP_Theme_JSON::LATEST_SCHEMA`).
pub const LATEST_SCHEMA: i64 = 3;
/// The selector style rules for the whole document apply to (`ROOT_BLOCK_SELECTOR`).
pub const ROOT_BLOCK_SELECTOR: &str = "body";
/// The selector CSS custom properties are declared on (`ROOT_CSS_PROPERTIES_SELECTOR`).
pub const ROOT_CSS_PROPERTIES_SELECTOR: &str = ":root";

/// The WordPress release the embedded `wordpress-<release>.*` files were exported from. `include_bytes!` takes only a
/// literal, so this macro is the one place the crate names the release; a test checks that it equals the theme-json
/// area's release (`compat/theme-json/source.json`'s override, else `compat/wordpress.json`'s pin). Moving the pin
/// edits this macro too.
macro_rules! data_release {
    () => {
        "7.2-RC2"
    };
}

const SERIALIZED: &[u8] = include_bytes!(concat!("wordpress-", data_release!(), ".constants"));

/// The release's `wp-includes/theme-i18n.json`: which settings and titles are translated, and with which
/// context.
pub const THEME_I18N: &str =
    include_str!(concat!("wordpress-", data_release!(), ".theme-i18n.json"));

const LAYOUT: &[u8] = include_bytes!(concat!("wordpress-", data_release!(), ".layout"));

thread_local! {
    static LAYOUT_DEFINITIONS: Rc<Value> = Rc::new(unserialize(LAYOUT));
    static CONSTANTS: Rc<Value> = Rc::new(load());
    static PRESETS: Rc<Vec<PresetMetadata>> = Rc::new(build_presets());
}

fn load() -> Value {
    let php = speedyspec_php_serialize::unserialize(SERIALIZED)
        .expect("the embedded constants unserialize");
    speedyspec_php_serialize::to_hook_value(&php)
}

fn unserialize(bytes: &[u8]) -> Value {
    let php =
        speedyspec_php_serialize::unserialize(bytes).expect("the embedded table unserializes");
    speedyspec_php_serialize::to_hook_value(&php)
}

/// `wp_get_layout_definitions()`: each layout type's class name, display mode and base and spacing rules
/// (exported from the release by `tools/export-layout-definitions.php`).
pub fn layout_definitions() -> Rc<Value> {
    LAYOUT_DEFINITIONS.with(Rc::clone)
}

/// Every class constant, as one PHP array in declaration order.
pub fn constants() -> Rc<Value> {
    CONSTANTS.with(Rc::clone)
}

/// The names of the class constants, in declaration order.
pub fn constant_names() -> Vec<String> {
    match &*constants() {
        Value::Array(entries) => entries
            .iter()
            .filter_map(|(k, _)| match k {
                Key::Str(s) => Some(s.clone()),
                Key::Int(_) => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

thread_local! {
    static TABLES: std::cell::RefCell<std::collections::HashMap<String, Rc<Value>>> = Default::default();
}

/// One class constant's value, shared (no copy): for readers that only look.
pub fn table(name: &str) -> Rc<Value> {
    TABLES.with(|tables| {
        if let Some(found) = tables.borrow().get(name) {
            return found.clone();
        }
        let value = Rc::new(constant(name).unwrap_or(Value::Null));
        tables.borrow_mut().insert(name.to_owned(), value.clone());
        value
    })
}

/// One class constant's value.
pub fn constant(name: &str) -> Option<Value> {
    get(&constants(), name).cloned()
}

/// Reads `array[key]` from a PHP array value.
pub fn get<'a>(array: &'a Value, key: &str) -> Option<&'a Value> {
    match array {
        Value::Array(entries) => entries.iter().find_map(|(k, v)| match k {
            Key::Str(s) if s == key => Some(v),
            _ => None,
        }),
        _ => None,
    }
}

/// Which value function a preset uses instead of a value key (`PRESETS_METADATA[*]['value_func']`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueFunc {
    /// `wp_get_typography_font_size_value`.
    FontSize,
}

/// One entry of `PRESETS_METADATA`: where a preset lives and how it becomes CSS.
#[derive(Clone, Debug, PartialEq)]
pub struct PresetMetadata {
    /// The path under a settings node, for example `["color", "palette"]`.
    pub path: Vec<String>,
    /// The setting that, when false, lets this origin's presets override defaults; `None` for WordPress's `false`.
    pub prevent_override: Option<Vec<String>>,
    /// Whether presets without a name take the default preset's name.
    pub use_default_names: bool,
    /// The key holding the preset's value, when there is one.
    pub value_key: Option<String>,
    /// The function computing the preset's value, when there is one.
    pub value_func: Option<ValueFunc>,
    /// The CSS custom property name, with `$slug` in place of the slug.
    pub css_vars: String,
    /// The class name templates and the CSS property each sets.
    pub classes: Vec<(String, String)>,
    /// The CSS properties the preset can feed.
    pub properties: Vec<String>,
}

fn strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(entries)) => entries
            .iter()
            .filter_map(|(_, v)| match v {
                Value::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn build_presets() -> Vec<PresetMetadata> {
    let Some(Value::Array(entries)) = constant("PRESETS_METADATA") else {
        return Vec::new();
    };
    entries
        .iter()
        .map(|(_, m)| PresetMetadata {
            path: strings(get(m, "path")),
            prevent_override: match get(m, "prevent_override") {
                Some(Value::Array(_)) => Some(strings(get(m, "prevent_override"))),
                _ => None,
            },
            use_default_names: matches!(get(m, "use_default_names"), Some(Value::Bool(true))),
            value_key: string(get(m, "value_key")),
            value_func: match get(m, "value_func") {
                Some(Value::Str(s)) if s == "wp_get_typography_font_size_value" => {
                    Some(ValueFunc::FontSize)
                }
                _ => None,
            },
            css_vars: string(get(m, "css_vars")).unwrap_or_default(),
            classes: match get(m, "classes") {
                Some(Value::Array(c)) => c
                    .iter()
                    .filter_map(|(k, v)| match (k, v) {
                        (Key::Str(class), Value::Str(property)) => {
                            Some((class.clone(), property.clone()))
                        }
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            },
            properties: strings(get(m, "properties")),
        })
        .collect()
}

/// `PRESETS_METADATA`, typed.
pub fn presets_metadata() -> Rc<Vec<PresetMetadata>> {
    PRESETS.with(Rc::clone)
}

/// The keys of `ELEMENTS`, in order.
pub fn element_names() -> Vec<String> {
    match constant("ELEMENTS") {
        Some(Value::Array(entries)) => entries
            .iter()
            .filter_map(|(k, _)| match k {
                Key::Str(s) => Some(s.clone()),
                Key::Int(_) => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_files_come_from_the_theme_json_areas_release() {
        let compat = concat!(env!("CARGO_MANIFEST_DIR"), "/../../compat");
        let json = |path: String| -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).expect(&path)
        };
        let source = json(format!("{compat}/theme-json/source.json"));
        let table = json(format!("{compat}/wordpress.json"));
        let release = source["wordpress"].as_str().or(table["wordpress"].as_str());
        assert_eq!(
            release,
            Some(data_release!()),
            "the theme-json area is recorded from WordPress {release:?}, not {}: re-export the \
             wordpress-<release>.* files with tools/ and change data_release!",
            data_release!()
        );
    }

    #[test]
    fn presets_metadata_order() {
        let paths: Vec<String> = presets_metadata()
            .iter()
            .map(|m| m.path.join("."))
            .collect();
        assert_eq!(
            paths,
            [
                "dimensions.aspectRatios",
                "color.palette",
                "color.gradients",
                "color.duotone",
                "typography.fontSizes",
                "typography.fontFamilies",
                "typography.textShadowPresets",
                "spacing.spacingSizes",
                "shadow.presets",
                "border.radiusSizes",
                "dimensions.dimensionSizes",
            ]
        );
    }

    #[test]
    fn preset_metadata_fields() {
        let all = presets_metadata();
        let palette = &all[1];
        assert_eq!(
            palette.prevent_override.as_deref(),
            Some(&["color".to_owned(), "defaultPalette".to_owned()][..])
        );
        assert_eq!(palette.value_key.as_deref(), Some("color"));
        assert_eq!(palette.css_vars, "--wp--preset--color--$slug");
        let font_sizes = &all[4];
        assert_eq!(font_sizes.value_func, Some(ValueFunc::FontSize));
        assert_eq!(font_sizes.value_key, None);
        assert_eq!(all[5].prevent_override, None);
        assert!(all[3].value_func.is_none() && all[3].value_key.is_none());
    }

    #[test]
    fn scalar_constants() {
        assert_eq!(LATEST_SCHEMA, 3);
        assert_eq!(ROOT_BLOCK_SELECTOR, "body");
        assert_eq!(ROOT_CSS_PROPERTIES_SELECTOR, ":root");
        assert_eq!(constant("LATEST_SCHEMA"), Some(Value::Int(3)));
    }

    #[test]
    fn valid_settings_has_custom_leaf() {
        let settings = constant("VALID_SETTINGS").unwrap();
        assert_eq!(get(&settings, "custom"), Some(&Value::Null));
    }

    #[test]
    fn constant_names_in_declaration_order() {
        let names = constant_names();
        assert_eq!(
            names.first().map(String::as_str),
            Some("ROOT_CSS_PROPERTIES_SELECTOR")
        );
        assert_eq!(names.last().map(String::as_str), Some("LATEST_SCHEMA"));
        assert_eq!(names.len(), 20);
    }
}
