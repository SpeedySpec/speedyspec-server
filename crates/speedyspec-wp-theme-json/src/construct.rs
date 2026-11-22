//! Constructing a document: `new WP_Theme_JSON( $theme_json, $origin )`.

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::model::ThemeJson;
use speedyspec_wp_theme_json_contracts::schema::{self, presets_metadata};
use speedyspec_wp_theme_json_contracts::Origin;

use crate::array_path;
use crate::presets::spacing::{compute_spacing_sizes, merge_spacing_sizes};
use crate::sanitize::{ValidNames, opt_in_into_settings, sanitize, unwrap_shared_block_style_variations};
use crate::schema_migrate::migrate;
use crate::{ThemeJsonCodec, ThemeJsonDeps};

/// The names sanitizing allows, from the registered blocks.
pub fn valid_names(deps: &ThemeJsonDeps) -> ValidNames {
    let blocks = deps.blocks.registered();
    ValidNames::new(
        blocks.iter().map(|b| b.name.clone()).collect(),
        schema::element_names(),
        blocks.iter().filter(|b| !b.style_names.is_empty()).map(|b| (b.name.clone(), b.style_names.clone())).collect(),
    )
}

/// The setting nodes' paths: the root settings and each block's (`get_setting_nodes` without selectors).
pub(crate) fn setting_paths(doc: &Value) -> Vec<Vec<String>> {
    if !array_path::isset(doc, &["settings"]) {
        return Vec::new();
    }
    let mut paths = vec![vec!["settings".to_owned()]];
    if array_path::isset(doc, &["settings", "blocks"])
        && let Some(Value::Array(blocks)) = array_path::get(doc, &["settings", "blocks"])
    {
        for (name, _) in blocks {
            let name = match name {
                Key::Str(s) => s.clone(),
                Key::Int(i) => i.to_string(),
            };
            paths.push(vec!["settings".to_owned(), "blocks".to_owned(), name]);
        }
    }
    paths
}

/// `isset( $preset[0] ) || empty( $preset )`: the preset is not keyed by origin yet.
fn not_keyed(preset: &Value) -> bool {
    match preset {
        Value::Array(e) => e.is_empty() || e.iter().any(|(k, v)| *k == Key::Int(0) && !matches!(v, Value::Null)),
        Value::Str(s) => !s.is_empty(),
        Value::Bytes(b) => !b.is_empty(),
        Value::Bool(b) => !b,
        Value::Int(i) => *i == 0,
        Value::Float(f) => *f == 0.0,
        Value::Null | Value::Opaque(_) => false,
    }
}

/// `WP_Theme_JSON::__construct( $theme_json, $origin )` as a PHP array: migrate, unwrap shared variations,
/// sanitize, opt into settings, key presets and the spacing scale by origin, and pre-generate spacing sizes.
pub fn construct_value(doc: Value, origin: &[u8], deps: &ThemeJsonDeps) -> Value {
    let origin = Origin::from_wp(origin);
    let valid = valid_names(deps);
    let doc = migrate(doc, origin);
    let doc = unwrap_shared_block_style_variations(doc, &valid);
    let doc = sanitize(&doc, &valid);
    let mut doc = opt_in_into_settings(doc);

    let wrap = |value: Value| Value::Array(vec![(Key::Str(origin.as_str().into()), value)]);
    for node in setting_paths(&doc) {
        for metadata in presets_metadata().iter() {
            let path: Vec<&str> = node.iter().map(String::as_str).chain(metadata.path.iter().map(String::as_str)).collect();
            if let Some(preset) = array_path::get(&doc, &path).filter(|p| !matches!(p, Value::Null))
                && not_keyed(preset)
            {
                let preset = preset.clone();
                array_path::set(&mut doc, &path, wrap(preset));
            }
        }
    }

    let scale_path = ["settings", "spacing", "spacingScale"];
    if let Some(scale) = array_path::get(&doc, &scale_path).filter(|s| !matches!(s, Value::Null)) {
        let keyed = match scale {
            Value::Array(e) => e.iter().any(|(k, _)| matches!(k, Key::Str(s) if Origin::parse(s.as_bytes()).is_some())),
            _ => false,
        };
        if !keyed {
            let scale = scale.clone();
            array_path::set(&mut doc, &scale_path, wrap(scale));
        }
    }

    let origin_scale = ["settings", "spacing", "spacingScale", origin.as_str()];
    if let Some(scale) = array_path::get(&doc, &origin_scale).filter(|s| !matches!(s, Value::Null)) {
        let sizes_path = ["settings", "spacing", "spacingSizes", origin.as_str()];
        let sizes = array_path::get(&doc, &sizes_path).cloned().unwrap_or(Value::Array(Vec::new()));
        let generated = compute_spacing_sizes(scale, deps);
        let merged = merge_spacing_sizes(&generated, &sizes);
        array_path::set(&mut doc, &sizes_path, merged);
    }
    doc
}

/// `new WP_Theme_JSON( $theme_json, $origin )`.
pub fn construct(doc: Value, origin: &[u8], deps: &ThemeJsonDeps) -> ThemeJson {
    ThemeJsonCodec::decode(&construct_value(doc, origin, deps))
}
