//! Preset CSS variables and classes, and the `custom` variables: `get_settings_values_by_slug`,
//! `get_settings_slugs`, `compute_preset_vars`, `compute_theme_vars`, `flatten_tree` and `compute_preset_classes`.

use speedyspec_hooks::Value;
use speedyspec_wp_theme_json_contracts::Origin;
use speedyspec_wp_theme_json_contracts::schema::{
    PresetMetadata, ROOT_BLOCK_SELECTOR, ROOT_CSS_PROPERTIES_SELECTOR, ValueFunc, presets_metadata,
};

use super::fluid::FluidFontSize;
use super::kebab::to_kebab_case;
use crate::array_path::{self, child};
use crate::css::{Declaration, to_ruleset};
use crate::php::to_php_string;
use crate::php_value::{entries, key, key_str};

/// `replace_slug_in_string( $input, $slug )`.
pub fn replace_slug_in_string(input: &str, slug: &str) -> String {
    input.replace("$slug", slug)
}

fn path_refs(metadata: &PresetMetadata) -> Vec<&str> {
    metadata.path.iter().map(String::as_str).collect()
}

fn slug_of(preset: &Value) -> String {
    to_kebab_case(&to_php_string(
        child(preset, &key("slug")).unwrap_or(&Value::Null),
    ))
}

/// `$array[ $slug ] = $value`: a later slug replaces an earlier one in place.
fn put(result: &mut Vec<(String, Value)>, slug: String, value: Value) {
    match result.iter_mut().find(|(s, _)| *s == slug) {
        Some(slot) => slot.1 = value,
        None => result.push((slug, value)),
    }
}

/// `get_settings_values_by_slug( $settings, $preset_metadata, $origins )`: each preset's value by kebab-cased
/// slug, for the given origins in order.
pub fn settings_values_by_slug(
    settings: &Value,
    metadata: &PresetMetadata,
    origins: &[Origin],
    fluid: &FluidFontSize,
) -> Vec<(String, Value)> {
    let per_origin = array_path::get(settings, &path_refs(metadata))
        .cloned()
        .unwrap_or(Value::Null);
    let mut result = Vec::new();
    for origin in origins {
        let Some(presets) =
            child(&per_origin, &key(origin.as_str())).filter(|v| !matches!(v, Value::Null))
        else {
            continue;
        };
        for (_, preset) in entries(presets) {
            let slug = slug_of(preset);
            let value = match (&metadata.value_key, metadata.value_func) {
                (Some(value_key), _) if array_path::isset(preset, &[value_key.as_str()]) => {
                    array_path::get(preset, &[value_key.as_str()])
                        .cloned()
                        .unwrap_or(Value::Null)
                }
                (_, Some(ValueFunc::FontSize)) => fluid.value(preset, settings),
                _ => continue,
            };
            put(&mut result, slug, value);
        }
    }
    result
}

/// `get_settings_slugs( $settings, $preset_metadata, $origins )`: the kebab-cased slugs, without repeats.
pub fn settings_slugs(
    settings: &Value,
    metadata: &PresetMetadata,
    origins: &[Origin],
) -> Vec<String> {
    let per_origin = array_path::get(settings, &path_refs(metadata))
        .cloned()
        .unwrap_or(Value::Null);
    let mut result: Vec<(String, Value)> = Vec::new();
    for origin in origins {
        let Some(presets) =
            child(&per_origin, &key(origin.as_str())).filter(|v| !matches!(v, Value::Null))
        else {
            continue;
        };
        for (_, preset) in entries(presets) {
            let slug = slug_of(preset);
            put(&mut result, slug, Value::Null);
        }
    }
    result.into_iter().map(|(s, _)| s).collect()
}

/// `compute_preset_vars( $settings, $origins )`.
pub fn compute_preset_vars(
    settings: &Value,
    origins: &[Origin],
    fluid: &FluidFontSize,
) -> Vec<Declaration> {
    let mut declarations = Vec::new();
    for metadata in presets_metadata().iter() {
        if metadata.css_vars.is_empty() {
            continue;
        }
        for (slug, value) in settings_values_by_slug(settings, metadata, origins, fluid) {
            declarations.push(Declaration::new(
                replace_slug_in_string(&metadata.css_vars, &slug),
                value,
            ));
        }
    }
    declarations
}

/// `flatten_tree( $tree, $prefix, $token )`.
pub fn flatten_tree(tree: &Value, prefix: &str, token: &str) -> Vec<(String, Value)> {
    let mut result: Vec<(String, Value)> = Vec::new();
    for (property, value) in entries(tree) {
        let new_key = format!(
            "{prefix}{}",
            to_kebab_case(key_str(property).as_bytes())
                .to_ascii_lowercase()
                .replace('/', "-")
        );
        if let Value::Array(_) = value {
            for (k, v) in flatten_tree(value, &format!("{new_key}{token}"), token) {
                put(&mut result, k, v);
            }
        } else {
            put(&mut result, new_key, value.clone());
        }
    }
    result
}

/// `compute_theme_vars( $settings )`: the `custom` settings as `--wp--custom--…` variables.
pub fn compute_theme_vars(settings: &Value) -> Vec<Declaration> {
    let custom = child(settings, &key("custom"))
        .filter(|v| !matches!(v, Value::Null))
        .cloned()
        .unwrap_or(Value::Array(Vec::new()));
    flatten_tree(&custom, "", "--")
        .into_iter()
        .map(|(k, v)| Declaration::new(format!("--wp--custom--{k}"), v))
        .collect()
}

/// `compute_preset_classes( $settings, $selector, $origins )`.
pub fn compute_preset_classes(settings: &Value, selector: &str, origins: &[Origin]) -> Vec<u8> {
    let selector = if selector == ROOT_BLOCK_SELECTOR || selector == ROOT_CSS_PROPERTIES_SELECTOR {
        ""
    } else {
        selector
    };
    let mut stylesheet = Vec::new();
    for metadata in presets_metadata().iter() {
        if metadata.classes.is_empty() {
            continue;
        }
        let slugs = settings_slugs(settings, metadata, origins);
        for (class, property) in &metadata.classes {
            for slug in &slugs {
                let css_var = replace_slug_in_string(&metadata.css_vars, slug);
                let class_name = replace_slug_in_string(class, slug);
                let new_selector = if selector.is_empty() {
                    class_name
                } else {
                    format!(":where({selector}){class_name}")
                };
                stylesheet.extend(to_ruleset(
                    &new_selector,
                    &[Declaration::text(
                        property.clone(),
                        format!("var({css_var}) !important"),
                    )],
                ));
            }
        }
    }
    stylesheet
}
