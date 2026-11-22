//! Sanitizing: what WordPress keeps of a theme.json document before anything reads it.
//!
//! The schema is a Specification built from the schema tables, the registered block names, the element names, the
//! registered block style variations and the document's own breakpoints, as `WP_Theme_JSON::sanitize` builds it.
//! It runs on the untyped value, because its input is arbitrary; its output is what the codec decodes.

mod css_format;
mod opt_in;
mod spec;
mod variations;
pub mod viewport;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::schema;

pub use css_format::{convert_custom_properties, resolve_custom_css_format};
pub use opt_in::opt_in_into_settings;
pub use spec::{Spec, remove_keys_not_in_schema};
pub use variations::unwrap_shared_block_style_variations;

use crate::array_path;
use crate::array_path::php_key;

/// The names the schema allows: registered blocks, elements, and each block's registered style variations.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ValidNames {
    /// Registered block names, in registration order.
    pub blocks: Vec<String>,
    /// Element names (`ELEMENTS`' keys).
    pub elements: Vec<String>,
    /// Each block's registered style variation names.
    pub variations: Vec<(String, Vec<String>)>,
}

impl ValidNames {
    /// Collects the names.
    pub fn new(
        blocks: Vec<String>,
        elements: Vec<String>,
        variations: Vec<(String, Vec<String>)>,
    ) -> ValidNames {
        ValidNames {
            blocks,
            elements,
            variations,
        }
    }

    /// The registered variation names of `block`, if it has any.
    pub fn variations_of(&self, block: &str) -> Option<&[String]> {
        self.variations
            .iter()
            .find(|(b, _)| b == block)
            .map(|(_, v)| v.as_slice())
    }
}

thread_local! {
    static VALID_STYLES: Spec = Spec::from_value(&schema::constant("VALID_STYLES").unwrap_or(Value::Null));
    static VALID_SETTINGS: Spec = Spec::from_value(&schema::constant("VALID_SETTINGS").unwrap_or(Value::Null));
    static FONT_FAMILY_SCHEMA: Spec = Spec::from_value(&schema::constant("FONT_FAMILY_SCHEMA").unwrap_or(Value::Null));
    static PSEUDO_ELEMENTS: Vec<(String, Vec<String>)> = string_lists("VALID_ELEMENT_PSEUDO_SELECTORS");
    static PSEUDO_BLOCKS: Vec<(String, Vec<String>)> = string_lists("VALID_BLOCK_PSEUDO_SELECTORS");
    static CUSTOM_STATES: Vec<(String, Vec<String>)> = string_lists("VALID_BLOCK_CUSTOM_STATES");
}

/// A constant of the shape `name => [strings]`.
fn string_lists(name: &str) -> Vec<(String, Vec<String>)> {
    let Some(Value::Array(entries)) = schema::constant(name) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|(k, v)| {
            let Key::Str(k) = k else { return None };
            let Value::Array(items) = v else { return None };
            let items = items.iter().filter_map(|(_, s)| {
                if let Value::Str(s) = s {
                    Some(s.clone())
                } else {
                    None
                }
            });
            Some((k.clone(), items.collect()))
        })
        .collect()
}

fn lookup<'a>(lists: &'a [(String, Vec<String>)], name: &str) -> Option<&'a [String]> {
    lists
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_slice())
}

fn key(name: &str) -> Key {
    php_key(name)
}

/// `VALID_STYLES` without the properties marked `'top'` (allowed at the top level only).
fn styles_non_top_level() -> Spec {
    let Some(Value::Array(sections)) = schema::constant("VALID_STYLES") else {
        return Spec::empty();
    };
    let filtered = sections
        .into_iter()
        .map(|(k, v)| match v {
            Value::Array(props) => (
                k,
                Value::Array(
                    props
                        .into_iter()
                        .filter(|(_, p)| !matches!(p, Value::Str(s) if s == "top"))
                        .collect(),
                ),
            ),
            other => (k, other),
        })
        .collect();
    Spec::from_value(&Value::Array(filtered))
}

/// The styles and settings schemas `sanitize` filters by.
struct Schemas {
    styles: Spec,
    settings: Spec,
}

fn build_schemas(input: &Value, valid: &ValidNames) -> Schemas {
    let non_top = styles_non_top_level();
    let breakpoints: Vec<String> =
        viewport::media_queries(array_path::get(input, &["settings", "viewport"]), false)
            .into_iter()
            .map(|(k, _)| k)
            .collect();

    let mut elements = Spec::empty();
    PSEUDO_ELEMENTS.with(|pseudo| {
        for element in &valid.elements {
            let mut schema = non_top.clone();
            for selector in lookup(pseudo, element).unwrap_or_default() {
                schema.set(key(selector), non_top.clone());
            }
            for state in &breakpoints {
                schema.set(key(state), non_top.clone());
            }
            elements.set(key(element), schema);
        }
    });

    let mut blocks_styles = Spec::empty();
    let mut blocks_settings = Spec::empty();
    let valid_settings = VALID_SETTINGS.with(Clone::clone);
    PSEUDO_BLOCKS.with(|pseudo| {
        CUSTOM_STATES.with(|custom| {
            for block in &valid.blocks {
                let mut settings = valid_settings.clone();
                settings.remove(&key("viewport"));
                settings.remove(&key("blockVisibility"));
                blocks_settings.set(key(block), settings);

                let pseudos = lookup(pseudo, block).unwrap_or_default();
                let mut styles = non_top.clone();
                styles.set(key("elements"), elements.clone());
                for state in &breakpoints {
                    let mut state_schema = non_top.clone();
                    state_schema.set(key("elements"), elements.clone());
                    for selector in pseudos {
                        state_schema.set(key(selector), non_top.clone());
                    }
                    styles.set(key(state), state_schema);
                }
                for selector in pseudos {
                    styles.set(key(selector), non_top.clone());
                }
                for state in lookup(custom, block).unwrap_or_default() {
                    let mut state_schema = non_top.clone();
                    for selector in pseudos {
                        state_schema.set(key(selector), non_top.clone());
                    }
                    styles.set(key(state), state_schema);
                }
                blocks_styles.set(key(block), styles);
            }
        });
    });

    let mut variation_styles = VALID_STYLES.with(Clone::clone);
    variation_styles.set(key("blocks"), blocks_styles.clone());
    variation_styles.set(key("elements"), elements.clone());

    PSEUDO_BLOCKS.with(|pseudo| {
        for block in &valid.blocks {
            let pseudos = lookup(pseudo, block).unwrap_or_default();
            let mut names: Vec<String> = Vec::new();
            if let (Some(Value::Array(given)), Some(registered)) = (
                array_path::get(input, &["styles", "blocks", block, "variations"]),
                valid.variations_of(block),
            ) && !given.is_empty()
            {
                names = given
                    .iter()
                    .filter_map(|(k, _)| {
                        let name = match k {
                            Key::Str(s) => s.clone(),
                            Key::Int(i) => i.to_string(),
                        };
                        registered.contains(&name).then_some(name)
                    })
                    .collect();
            }
            let mut variations = Spec::empty();
            for name in &names {
                let mut schema = variation_styles.clone();
                for state in &breakpoints {
                    let mut state_schema = non_top.clone();
                    state_schema.set(key("elements"), elements.clone());
                    // A snapshot: blocks earlier in this loop already carry their `variations`.
                    state_schema.set(key("blocks"), blocks_styles.clone());
                    for selector in pseudos {
                        state_schema.set(key(selector), non_top.clone());
                    }
                    schema.set(key(state), state_schema);
                }
                for selector in pseudos {
                    schema.set(key(selector), non_top.clone());
                }
                variations.set(key(name), schema);
            }
            blocks_styles
                .entry(key(block))
                .set(key("variations"), variations);
        }
    });

    let mut styles = VALID_STYLES.with(Clone::clone);
    styles.set(key("blocks"), blocks_styles);
    styles.set(key("elements"), elements);
    let mut settings = valid_settings;
    settings.set(key("blocks"), blocks_settings);
    let font_families = FONT_FAMILY_SCHEMA.with(Clone::clone);
    let mut per_origin = font_families.clone();
    for origin in speedyspec_wp_theme_json_contracts::Origin::ALL {
        per_origin.set(key(origin.as_str()), font_families.clone());
    }
    settings
        .entry(key("typography"))
        .set(key("fontFamilies"), per_origin);
    Schemas { styles, settings }
}

/// `WP_Theme_JSON::sanitize( $input, $valid_block_names, $valid_element_names, $valid_variations )`.
pub fn sanitize(input: &Value, valid: &ValidNames) -> Value {
    let Value::Array(entries) = input else {
        return Value::Array(Vec::new());
    };
    let top_level = schema::constant("VALID_TOP_LEVEL_KEYS").unwrap_or(Value::Null);
    let allowed = |k: &Key| match (&top_level, k) {
        (Value::Array(keys), Key::Str(name)) => keys
            .iter()
            .any(|(_, v)| matches!(v, Value::Str(s) if s == name)),
        _ => false,
    };
    let mut output = Value::Array(
        entries
            .iter()
            .filter(|(k, _)| allowed(k))
            .cloned()
            .collect(),
    );
    let schemas = build_schemas(input, valid);

    for (subtree, schema) in [("styles", &schemas.styles), ("settings", &schemas.settings)] {
        if !array_path::isset(input, &[subtree]) {
            continue;
        }
        let given = array_path::get(input, &[subtree])
            .cloned()
            .unwrap_or(Value::Null);
        if !matches!(given, Value::Array(_)) {
            array_path::unset(&mut output, &[subtree]);
            continue;
        }
        let mut result = remove_keys_not_in_schema(given.clone(), schema);
        if subtree == "settings" && array_path::child(&given, &key("viewport")).is_some() {
            let sanitized =
                viewport::sanitize_viewport_settings(array_path::child(&given, &key("viewport")));
            array_path::insert(&mut result, key("viewport"), sanitized);
        }
        if matches!(&result, Value::Array(e) if e.is_empty()) {
            array_path::unset(&mut output, &[subtree]);
        } else {
            array_path::insert(&mut output, key(subtree), resolve_custom_css_format(result));
        }
    }
    output
}
