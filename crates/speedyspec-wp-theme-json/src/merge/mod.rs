//! Merging one layer into another: `WP_Theme_JSON::merge( $incoming )`.
//!
//! WordPress merges by array algebra: `array_replace_recursive`, then the spacing sizes are regenerated from the
//! layers' scales, `spacing.units` and presets are replaced rather than merged (with the preset rules below), and
//! background images are replaced whole. That runs on the array form; the result is decoded back into the model.

mod rules;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::Origin;
use speedyspec_wp_theme_json_contracts::model::ThemeJson;
use speedyspec_wp_theme_json_contracts::schema::presets_metadata;

pub use rules::{PresetContext, PresetRule, RemoveDefaultSlugs, UseDefaultNames};

use crate::array_path::{self, replace_recursive};
use crate::construct::setting_paths;
use crate::presets::spacing::{compute_spacing_sizes, merge_spacing_sizes};
use crate::sanitize::viewport;
use crate::{ThemeJsonCodec, ThemeJsonDeps};

fn path_of<'a>(base: &'a [String], tail: &'a [&'a str]) -> Vec<&'a str> {
    base.iter()
        .map(String::as_str)
        .chain(tail.iter().copied())
        .collect()
}

fn array_or_empty(value: Option<&Value>) -> Value {
    match value {
        Some(v @ Value::Array(_)) => v.clone(),
        _ => Value::Array(Vec::new()),
    }
}

/// `array_replace( $a, $b )`: `b`'s keys replace `a`'s; new keys are appended.
fn array_replace(mut a: Vec<(Key, Value)>, b: &Value) -> Vec<(Key, Value)> {
    if let Value::Array(entries) = b {
        for (k, v) in entries {
            match a.iter_mut().find(|(ak, _)| ak == k) {
                Some(slot) => slot.1 = v.clone(),
                None => a.push((k.clone(), v.clone())),
            }
        }
    }
    a
}

/// The style node paths `merge` replaces background images at: each block, each block element that is set, then
/// the root (`get_block_nodes` with `include_node_paths_only`, plus `styles`).
fn style_node_paths(doc: &Value) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    if array_path::isset(doc, &["styles", "blocks"])
        && let Some(Value::Array(blocks)) = array_path::get(doc, &["styles", "blocks"])
    {
        let breakpoints =
            viewport::media_queries(array_path::get(doc, &["settings", "viewport"]), false);
        for (name, block) in blocks {
            let name = match name {
                Key::Str(s) => s.clone(),
                Key::Int(i) => i.to_string(),
            };
            paths.push(vec!["styles".into(), "blocks".into(), name.clone()]);
            let mut elements: Vec<Key> = Vec::new();
            let mut collect = |value: Option<&Value>| {
                if let Some(Value::Array(e)) = value {
                    for (k, _) in e {
                        if !elements.contains(k) {
                            elements.push(k.clone());
                        }
                    }
                }
            };
            collect(array_path::get(block, &["elements"]));
            for (state, _) in &breakpoints {
                collect(array_path::get(block, &[state, "elements"]));
            }
            for element in elements {
                let element = match element {
                    Key::Str(s) => s,
                    Key::Int(i) => i.to_string(),
                };
                if array_path::isset(block, &["elements", &element]) {
                    paths.push(vec![
                        "styles".into(),
                        "blocks".into(),
                        name.clone(),
                        "elements".into(),
                        element,
                    ]);
                }
            }
        }
    }
    paths.push(vec!["styles".into()]);
    paths
}

/// PHP's truthiness.
pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) | Value::Int(0) => false,
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => !(s.is_empty() || s == "0"),
        Value::Bytes(b) => !(b.is_empty() || b == b"0"),
        Value::Array(a) => !a.is_empty(),
        _ => true,
    }
}

/// `WP_Theme_JSON::merge()` on the array forms of the two documents.
pub fn merge_value(base: Value, incoming: Value, deps: &ThemeJsonDeps) -> Value {
    let mut incoming = incoming;
    let mut merged = replace_recursive(base, incoming.clone());

    let mut flattened: Vec<(Key, Value)> = Vec::new();
    for origin in Origin::ALL {
        let scale_path = ["settings", "spacing", "spacingScale", origin.as_str()];
        flattened = array_replace(
            flattened,
            &array_or_empty(array_path::get(&merged, &scale_path)),
        );
        let Some(scale) = array_path::get(&incoming, &scale_path)
            .filter(|s| !matches!(s, Value::Null))
            .cloned()
        else {
            continue;
        };
        flattened = array_replace(flattened, &scale);
        let sizes_path = ["settings", "spacing", "spacingSizes", origin.as_str()];
        let sizes = array_path::get(&incoming, &sizes_path)
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let generated = compute_spacing_sizes(&Value::Array(flattened.clone()), deps);
        array_path::set(
            &mut incoming,
            &sizes_path,
            merge_spacing_sizes(&generated, &sizes),
        );
    }

    let rules: [&dyn PresetRule; 2] = [&UseDefaultNames, &RemoveDefaultSlugs];
    let global_slugs = rules::default_slugs(&merged, &["settings".to_owned()]);
    for node in setting_paths(&incoming) {
        let units = path_of(&node, &["spacing", "units"]);
        if let Some(content) =
            array_path::get(&incoming, &units).filter(|c| !matches!(c, Value::Null))
        {
            let content = content.clone();
            array_path::set(&mut merged, &units, content);
        }
        for metadata in presets_metadata().iter() {
            let prevent_override = match &metadata.prevent_override {
                None => Value::Bool(false),
                Some(path) => {
                    let global: Vec<&str> = std::iter::once("settings")
                        .chain(path.iter().map(String::as_str))
                        .collect();
                    let global = array_path::get(&merged, &global)
                        .cloned()
                        .unwrap_or(Value::Null);
                    let node_level: Vec<&str> = path_of(&node, &[])
                        .into_iter()
                        .chain(path.iter().map(String::as_str))
                        .collect();
                    array_path::get(&merged, &node_level)
                        .cloned()
                        .unwrap_or(global)
                }
            };
            for origin in Origin::ALL {
                let base_path: Vec<String> = node
                    .iter()
                    .cloned()
                    .chain(metadata.path.iter().cloned())
                    .collect();
                let tail = [origin.as_str()];
                let path = path_of(&base_path, &tail);
                let Some(content) =
                    array_path::get(&incoming, &path).filter(|c| !matches!(c, Value::Null))
                else {
                    continue;
                };
                let context = PresetContext {
                    merged: &merged,
                    node: &node,
                    base_path: &base_path,
                    metadata,
                    origin,
                    prevent_override: truthy(&prevent_override),
                    global_slugs: &global_slugs,
                };
                let content = rules.iter().fold(content.clone(), |content, rule| {
                    if rule.applies(&context) {
                        rule.apply(&context, content)
                    } else {
                        content
                    }
                });
                array_path::set(&mut merged, &path, content);
            }
        }
    }

    for node in style_node_paths(&merged) {
        let path = path_of(&node, &["background", "backgroundImage"]);
        if let Some(content) =
            array_path::get(&incoming, &path).filter(|c| !matches!(c, Value::Null))
        {
            let content = content.clone();
            array_path::set(&mut merged, &path, content);
        }
    }
    merged
}

/// `$base->merge( $incoming )`, returning the merged document.
pub fn merge(base: &ThemeJson, incoming: &ThemeJson, deps: &ThemeJsonDeps) -> ThemeJson {
    ThemeJsonCodec::decode(&merge_value(
        ThemeJsonCodec::encode(base),
        ThemeJsonCodec::encode(incoming),
        deps,
    ))
}
