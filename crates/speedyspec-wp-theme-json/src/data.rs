//! The data methods of `WP_Theme_JSON` that read a document back out: `get_data` (the export form), templates,
//! template parts, editor settings and element class names.

use std::cmp::Ordering;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::schema::{self, presets_metadata};

use crate::array_path::{self, child, php_key};
use crate::construct::setting_paths;
use crate::php::{numeric_string, to_number, to_php_string};

fn key(name: &str) -> Key {
    Key::Str(name.to_owned())
}

fn list(items: Vec<Value>) -> Value {
    Value::Array(items.into_iter().enumerate().map(|(i, v)| (Key::Int(i as i64), v)).collect())
}

/// `$array[ $value ]`'s key for a slug value.
fn value_key(value: Option<&Value>) -> Key {
    match value {
        Some(Value::Int(i)) => Key::Int(*i),
        Some(Value::Float(f)) => Key::Int(*f as i64),
        Some(Value::Bool(b)) => Key::Int(i64::from(*b)),
        Some(v @ (Value::Str(_) | Value::Bytes(_))) => php_key(&String::from_utf8_lossy(&to_php_string(v))),
        _ => Key::Str(String::new()),
    }
}

fn key_string(key: &Key) -> String {
    match key {
        Key::Int(i) => i.to_string(),
        Key::Str(s) => s.clone(),
    }
}

/// `get_data()`'s preset flattening: theme then custom presets, keyed by slug (a later one replaces an earlier one in
/// place), each written back with its slug first, as a string.
fn flatten_preset(preset: &Value) -> Value {
    let mut items: Vec<(Key, Vec<(Key, Value)>)> = Vec::new();
    for origin in ["theme", "custom"] {
        let Some(Value::Array(entries)) = child(preset, &key(origin)).filter(|v| !matches!(v, Value::Null)) else {
            continue;
        };
        for (_, item) in entries {
            let slug = value_key(child(item, &key("slug")));
            let rest = match item {
                Value::Array(fields) => fields.iter().filter(|(k, _)| *k != key("slug")).cloned().collect(),
                _ => Vec::new(),
            };
            match items.iter_mut().find(|(k, _)| *k == slug) {
                Some(slot) => slot.1 = rest,
                None => items.push((slug, rest)),
            }
        }
    }
    list(
        items
            .into_iter()
            .map(|(slug, rest)| {
                let mut fields = vec![(key("slug"), Value::Str(key_string(&slug)))];
                let mut next_int = 0;
                for (k, v) in rest {
                    match k {
                        Key::Int(_) => {
                            fields.push((Key::Int(next_int), v));
                            next_int += 1;
                        }
                        k => fields.push((k, v)),
                    }
                }
                Value::Array(fields)
            })
            .collect(),
    )
}

fn opt_in_paths() -> Vec<Vec<String>> {
    let Some(Value::Array(paths)) = schema::constant("APPEARANCE_TOOLS_OPT_INS") else { return Vec::new() };
    paths
        .iter()
        .filter_map(|(_, p)| match p {
            Value::Array(parts) => Some(parts.iter().filter_map(|(_, s)| if let Value::Str(s) = s { Some(s.clone()) } else { None }).collect()),
            _ => None,
        })
        .collect()
}

/// `WP_Theme_JSON::get_data()`: the document as a theme would write it.
pub fn get_data(raw: &Value) -> Value {
    let mut output = raw.clone();
    let nodes = setting_paths(&output);
    for node in &nodes {
        for metadata in presets_metadata().iter() {
            let path: Vec<&str> = node.iter().map(String::as_str).chain(metadata.path.iter().map(String::as_str)).collect();
            let Some(preset) = array_path::get(&output, &path).filter(|p| !matches!(p, Value::Null)) else { continue };
            let flattened = flatten_preset(preset);
            array_path::set(&mut output, &path, flattened);
        }
    }
    let opt_ins = opt_in_paths();
    for node in &nodes {
        let full = |opt: &[String]| -> Vec<String> { node.iter().chain(opt.iter()).cloned().collect() };
        let all_set = opt_ins.iter().all(|opt| {
            let path = full(opt);
            array_path::get(&output, &path.iter().map(String::as_str).collect::<Vec<_>>()).is_some()
        });
        if !all_set {
            continue;
        }
        let at: Vec<String> = full(&["appearanceTools".to_owned()]);
        array_path::set(&mut output, &at.iter().map(String::as_str).collect::<Vec<_>>(), Value::Bool(true));
        for opt in &opt_ins {
            let path = full(opt);
            let path: Vec<&str> = path.iter().map(String::as_str).collect();
            if !matches!(array_path::get(&output, &path), Some(Value::Bool(true))) {
                continue;
            }
            let is_root = node.len() == 1 && node[0] == "settings";
            let is_block = node.len() == 3 && node[0] == "settings" && node[1] == "blocks";
            if is_root || is_block {
                array_path::unset(&mut output, &path);
                let section = &path[..path.len() - 1];
                if array_path::get(&output, section).is_none_or(|s| !crate::merge::truthy(s)) {
                    array_path::unset(&mut output, section);
                }
            }
        }
    }
    recursive_ksort(output)
}

/// PHP 8's `SORT_REGULAR` comparison of two array keys.
fn compare_keys(a: &Key, b: &Key) -> Ordering {
    match (a, b) {
        (Key::Int(x), Key::Int(y)) => x.cmp(y),
        (Key::Str(x), Key::Str(y)) => match (numeric_string(x.as_bytes()), numeric_string(y.as_bytes())) {
            (Some(n), Some(m)) => n.as_f64().partial_cmp(&m.as_f64()).unwrap_or(Ordering::Equal),
            _ => x.as_bytes().cmp(y.as_bytes()),
        },
        (Key::Int(x), Key::Str(y)) => match numeric_string(y.as_bytes()) {
            Some(m) => (*x as f64).partial_cmp(&m.as_f64()).unwrap_or(Ordering::Equal),
            None => x.to_string().as_bytes().cmp(y.as_bytes()),
        },
        (Key::Str(_), Key::Int(_)) => compare_keys(b, a).reverse(),
    }
}

/// `wp_recursive_ksort()`.
pub fn recursive_ksort(value: Value) -> Value {
    match value {
        Value::Array(entries) => {
            let mut entries: Vec<(Key, Value)> = entries.into_iter().map(|(k, v)| (k, recursive_ksort(v))).collect();
            entries.sort_by(|(a, _), (b, _)| compare_keys(a, b));
            Value::Array(entries)
        }
        other => other,
    }
}

fn named_items(raw: &Value, top: &str, fields: &[(&str, Value)]) -> Value {
    if !array_path::isset(raw, &[top]) {
        return Value::Array(Vec::new());
    }
    let Some(Value::Array(items)) = array_path::get(raw, &[top]) else { return Value::Array(Vec::new()) };
    let mut out: Vec<(Key, Value)> = Vec::new();
    for (_, item) in items {
        let Some(name) = child(item, &key("name")).filter(|n| !matches!(n, Value::Null)) else { continue };
        let entry = Value::Array(
            fields
                .iter()
                .map(|(field, default)| {
                    let value = child(item, &key(field)).filter(|v| !matches!(v, Value::Null)).cloned();
                    (key(field), value.unwrap_or_else(|| default.clone()))
                })
                .collect(),
        );
        let k = value_key(Some(name));
        match out.iter_mut().find(|(ok, _)| *ok == k) {
            Some(slot) => slot.1 = entry,
            None => out.push((k, entry)),
        }
    }
    Value::Array(out)
}

/// `get_custom_templates()`: each template by name, with its title and post types (`['page']` by default).
pub fn custom_templates(raw: &Value) -> Value {
    named_items(raw, "customTemplates", &[("title", Value::Str(String::new())), ("postTypes", list(vec![Value::Str("page".into())]))])
}

/// `get_template_parts()`: each template part by name, with its title and area.
pub fn template_parts(raw: &Value) -> Value {
    named_items(raw, "templateParts", &[("title", Value::Str(String::new())), ("area", Value::Str(String::new()))])
}

/// `get_patterns()`.
pub fn patterns(raw: &Value) -> Value {
    match array_path::get(raw, &["patterns"]) {
        Some(p @ Value::Array(_)) => p.clone(),
        _ => Value::Array(Vec::new()),
    }
}

/// `get_settings()`.
pub fn settings(raw: &Value) -> Value {
    match array_path::get(raw, &["settings"]) {
        Some(Value::Null) | None => Value::Array(Vec::new()),
        Some(s) => s.clone(),
    }
}

/// `WP_Theme_JSON::get_element_class_name( $element )`.
pub fn element_class_name(element: &[u8]) -> Vec<u8> {
    let names = schema::constant("__EXPERIMENTAL_ELEMENT_CLASS_NAMES").unwrap_or(Value::Null);
    match child(&names, &php_key(&String::from_utf8_lossy(element))) {
        Some(Value::Str(s)) => s.as_bytes().to_vec(),
        _ => Vec::new(),
    }
}

/// `WP_Theme_JSON::get_from_editor_settings( $settings )`: a theme.json document from the block editor's settings.
pub fn from_editor_settings(settings: &Value) -> Value {
    let mut out = Value::Array(vec![(key("version"), Value::Int(schema::LATEST_SCHEMA)), (key("settings"), Value::Array(Vec::new()))]);
    let given = |name: &str| child(settings, &key(name)).filter(|v| !matches!(v, Value::Null));
    let negated = |v: &Value| Value::Bool(!crate::merge::truthy(v));
    if let Some(v) = given("disableCustomColors") {
        array_path::set(&mut out, &["settings", "color", "custom"], negated(v));
    }
    if let Some(v) = given("disableCustomGradients") {
        array_path::set(&mut out, &["settings", "color", "customGradient"], negated(v));
    }
    if let Some(v) = given("disableCustomFontSizes") {
        array_path::set(&mut out, &["settings", "typography", "customFontSize"], negated(v));
    }
    if let Some(v) = given("enableCustomLineHeight") {
        array_path::set(&mut out, &["settings", "typography", "lineHeight"], v.clone());
    }
    if let Some(v) = given("enableCustomUnits") {
        let units = if matches!(v, Value::Bool(true)) {
            list(["px", "em", "rem", "vh", "vw", "%"].iter().map(|u| Value::Str((*u).into())).collect())
        } else {
            v.clone()
        };
        array_path::set(&mut out, &["settings", "spacing", "units"], units);
    }
    if let Some(v) = given("colors") {
        array_path::set(&mut out, &["settings", "color", "palette"], v.clone());
    }
    if let Some(v) = given("gradients") {
        array_path::set(&mut out, &["settings", "color", "gradients"], v.clone());
    }
    if let Some(v) = given("fontSizes") {
        let sizes = match v {
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|(k, item)| {
                        let mut item = item.clone();
                        if let Some(size) = child(&item, &key("size")).filter(|s| to_number(s).is_some()) {
                            let mut text = to_php_string(size);
                            text.extend_from_slice(b"px");
                            array_path::insert(&mut item, key("size"), crate::codec::string_value(&text));
                        }
                        (k.clone(), item)
                    })
                    .collect(),
            ),
            other => other.clone(),
        };
        array_path::set(&mut out, &["settings", "typography", "fontSizes"], sizes);
    }
    if let Some(v) = given("enableCustomSpacing") {
        array_path::set(&mut out, &["settings", "spacing", "padding"], v.clone());
    }
    if let Some(v) = given("spacingSizes") {
        array_path::set(&mut out, &["settings", "spacing", "spacingSizes"], v.clone());
    }
    out
}
