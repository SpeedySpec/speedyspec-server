//! The stylesheet (a Builder over the pieces): `get_stylesheet` and the methods it is made of —
//! `get_css_variables`, `get_root_layout_rules`, `get_block_classes`, `get_preset_classes` and the custom CSS.

mod block;
mod custom_css;
mod states;

use custom_css::css_text;
pub use custom_css::process_blocks_custom_css;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::schema::{ROOT_BLOCK_SELECTOR, presets_metadata};
use speedyspec_wp_theme_json_contracts::{Origin, ThemeJsonError};

use crate::array_path;
use crate::css::{Declaration, to_ruleset};
use crate::nodes::{NodeOptions, setting_nodes, style_nodes_keyed};
use crate::php::to_php_string;
use crate::php_value::{as_str, empty, entries, key_str};
use crate::presets::vars::{
    compute_preset_classes, compute_theme_vars, replace_slug_in_string, settings_values_by_slug,
};
use crate::selectors::scope_selector;
use crate::sheet::Sheet;

/// What `get_stylesheet` outputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StylesheetType {
    /// `variables`: preset and custom CSS variables.
    Variables,
    /// `styles`: root layout rules and every style node's rules.
    Styles,
    /// `presets`: the preset classes.
    Presets,
    /// `custom-css`: the document's own CSS.
    CustomCss,
}

impl StylesheetType {
    fn parse(name: &str) -> Option<StylesheetType> {
        match name {
            "variables" => Some(StylesheetType::Variables),
            "styles" => Some(StylesheetType::Styles),
            "presets" => Some(StylesheetType::Presets),
            "custom-css" => Some(StylesheetType::CustomCss),
            _ => None,
        }
    }
}

/// `get_stylesheet`'s `$types`: a list, or a pre-5.9 string (deprecated, mapped as WordPress maps it).
pub fn stylesheet_types(types: &Value, sheet: &Sheet<'_>) -> Vec<StylesheetType> {
    if let Some(legacy) = as_str(types) {
        sheet
            .deps
            .diagnostics
            .deprecated_argument("get_stylesheet", "5.9.0", b"");
        return match legacy.as_ref() {
            "block_styles" => vec![StylesheetType::Styles, StylesheetType::Presets],
            "css_variables" => vec![StylesheetType::Variables],
            _ => vec![
                StylesheetType::Variables,
                StylesheetType::Styles,
                StylesheetType::Presets,
            ],
        };
    }
    entries(types)
        .iter()
        .filter_map(|(_, v)| as_str(v).and_then(|s| StylesheetType::parse(&s)))
        .collect()
}

/// `get_stylesheet`'s `$origins`: `null` is every origin.
pub fn stylesheet_origins(origins: &Value) -> Vec<Origin> {
    match origins {
        Value::Null => Origin::ALL.to_vec(),
        other => entries(other)
            .iter()
            .filter_map(|(_, v)| as_str(v).and_then(|s| Origin::parse(s.as_bytes())))
            .collect(),
    }
}

fn text(value: &Value) -> String {
    String::from_utf8_lossy(&to_php_string(value)).into_owned()
}

fn get_opt<'v>(value: &'v Value, path: &[&str]) -> Option<&'v Value> {
    array_path::get(value, path).filter(|v| !matches!(v, Value::Null))
}

fn path_of(node: &Value) -> Vec<String> {
    entries(array_path::get(node, &["path"]).unwrap_or(&Value::Null))
        .iter()
        .map(|(_, v)| text(v))
        .collect()
}

/// `get_feature_selector( $feature_selectors, $feature_key, $default_selector )`.
fn feature_selector(feature_selectors: &Value, feature: &str, default: &str) -> String {
    match get_opt(feature_selectors, &[feature]) {
        None => default.to_owned(),
        Some(Value::Str(s)) => s.clone(),
        Some(f) => match array_path::get(f, &["root"]) {
            Some(Value::Str(root)) => root.clone(),
            _ => default.to_owned(),
        },
    }
}

/// `scope_style_node_selectors( $scope, $node )`.
fn scope_style_node(scope: &str, mut node: Value) -> Value {
    let selector = get_opt(&node, &["selector"]).map(text);
    let scoped = match selector {
        Some(s) => Value::Str(scope_selector(scope, &s)),
        None => Value::Null,
    };
    array_path::insert(&mut node, Key::Str("selector".into()), scoped);
    let selectors = array_path::get(&node, &["selectors"])
        .cloned()
        .unwrap_or(Value::Null);
    if empty(&selectors) {
        return node;
    }
    let mut scoped_selectors = selectors.clone();
    for (feature, selector) in entries(&selectors) {
        match selector {
            Value::Str(s) => array_path::insert(
                &mut scoped_selectors,
                feature.clone(),
                Value::Str(scope_selector(scope, s)),
            ),
            Value::Array(subs) => {
                let mut scoped_subs = selector.clone();
                for (sub, sub_selector) in subs {
                    let scoped = match sub_selector {
                        Value::Null => Value::Null,
                        other => Value::Str(scope_selector(scope, &text(other))),
                    };
                    array_path::insert(&mut scoped_subs, sub.clone(), scoped);
                }
                array_path::insert(&mut scoped_selectors, feature.clone(), scoped_subs);
            }
            _ => {}
        }
    }
    array_path::insert(&mut node, Key::Str("selectors".into()), scoped_selectors);
    node
}

fn set_selector(node: &mut Value, selector: Value) {
    array_path::insert(node, Key::Str("selector".into()), selector);
}

/// `array_search( $selector, array_column( $nodes, 'selector' ), true )`: an index into the column, which skips
/// nodes without a `selector` key, as WordPress's does.
fn root_key(nodes: &[Value], selector: &str) -> Option<usize> {
    nodes
        .iter()
        .filter(|n| matches!(n, Value::Array(e) if e.iter().any(|(k, _)| *k == Key::Str("selector".into()))))
        .position(|n| matches!(array_path::get(n, &["selector"]), Some(Value::Str(s)) if s == selector))
}

impl Sheet<'_> {
    /// `get_css_variables( $nodes, $origins )`.
    pub fn css_variables(&self, nodes: &[Value], origins: &[Origin]) -> Vec<u8> {
        let mut stylesheet = Vec::new();
        for metadata in nodes {
            let Some(selector) = get_opt(metadata, &["selector"]).map(text) else {
                continue;
            };
            let feature_selectors = get_opt(metadata, &["selectors"])
                .cloned()
                .unwrap_or(Value::Array(Vec::new()));
            let path = path_of(metadata);
            let refs: Vec<&str> = path.iter().map(String::as_str).collect();
            let node = array_path::get(self.doc, &refs)
                .cloned()
                .unwrap_or(Value::Array(Vec::new()));
            let mut by_selector: Vec<(String, Vec<Declaration>)> =
                vec![(selector.clone(), Vec::new())];
            for preset in presets_metadata().iter() {
                if preset.css_vars.is_empty() {
                    continue;
                }
                let values = settings_values_by_slug(&node, preset, origins, &self.fluid);
                if values.is_empty() {
                    continue;
                }
                let target = feature_selector(
                    &feature_selectors,
                    preset.path.first().map_or("", String::as_str),
                    &selector,
                );
                let index = match by_selector.iter().position(|(s, _)| *s == target) {
                    Some(i) => i,
                    None => {
                        by_selector.push((target, Vec::new()));
                        by_selector.len() - 1
                    }
                };
                for (slug, value) in values {
                    by_selector[index].1.push(Declaration::new(
                        replace_slug_in_string(&preset.css_vars, &slug),
                        value,
                    ));
                }
            }
            let index = by_selector
                .iter()
                .position(|(s, _)| *s == selector)
                .unwrap_or(0);
            by_selector[index].1.extend(compute_theme_vars(&node));
            for (rule_selector, declarations) in &by_selector {
                stylesheet.extend(to_ruleset(rule_selector, declarations));
            }
        }
        stylesheet
    }

    /// `get_preset_classes( $setting_nodes, $origins )`.
    pub fn preset_classes(&self, nodes: &[Value], origins: &[Origin]) -> Vec<u8> {
        let mut rules = Vec::new();
        for metadata in nodes {
            let Some(selector) = get_opt(metadata, &["selector"]).map(text) else {
                continue;
            };
            let path = path_of(metadata);
            let refs: Vec<&str> = path.iter().map(String::as_str).collect();
            let node = array_path::get(self.doc, &refs)
                .cloned()
                .unwrap_or(Value::Array(Vec::new()));
            rules.extend(compute_preset_classes(&node, &selector, origins));
        }
        rules
    }

    /// `get_block_classes( $style_nodes )`.
    pub fn block_classes(&self, nodes: &[Value]) -> Vec<u8> {
        let mut rules = Vec::new();
        for metadata in nodes {
            if get_opt(metadata, &["selector"]).is_none() {
                continue;
            }
            rules.extend(self.styles_for_block(metadata));
        }
        rules
    }

    /// `get_stylesheet( $types, $origins, $options )`.
    pub fn stylesheet(
        &self,
        types: &[StylesheetType],
        origins: &[Origin],
        options: &Value,
    ) -> Result<Vec<u8>, ThemeJsonError> {
        let node_options = NodeOptions {
            include_block_style_variations: get_opt(options, &["include_block_style_variations"])
                .is_some_and(|v| !empty(v)),
            include_node_paths_only: get_opt(options, &["include_node_paths_only"])
                .is_some_and(|v| !empty(v)),
        };
        let mut styles =
            style_nodes_keyed(self.doc, &self.meta, node_options, &*self.deps.filters)?;
        let mut settings = setting_nodes(self.doc, &self.meta);
        // A position in `array_column( $style_nodes, 'selector' )`, used by WordPress as a key into the (possibly
        // filtered, keyed) node array.
        let column: Vec<Value> = styles.iter().map(|(_, v)| v.clone()).collect();
        let root_style_key = root_key(&column, ROOT_BLOCK_SELECTOR).map(|k| Key::Int(k as i64));
        // WordPress looks for `body` among the setting nodes too (their root is `:root`), so `root_selector` never
        // reaches the variables.
        let root_settings_key = root_key(&settings, ROOT_BLOCK_SELECTOR);
        if let Some(scope) = get_opt(options, &["scope"]).filter(|s| !empty(s)).map(text) {
            for node in settings.iter_mut() {
                let scoped = match get_opt(node, &["selector"]).map(text) {
                    Some(s) => Value::Str(scope_selector(&scope, &s)),
                    None => Value::Null,
                };
                set_selector(node, scoped);
            }
            styles = styles
                .into_iter()
                .map(|(k, n)| (k, scope_style_node(&scope, n)))
                .collect();
        }
        if let Some(root_selector) = get_opt(options, &["root_selector"])
            .filter(|s| !empty(s))
            .cloned()
        {
            if let Some(k) = root_settings_key.filter(|k| *k < settings.len()) {
                set_selector(&mut settings[k], root_selector.clone());
            }
            if let Some(k) = &root_style_key {
                match styles.iter_mut().find(|(key, _)| key == k) {
                    Some((_, node)) => set_selector(node, root_selector),
                    // `$style_nodes[ $key ]['selector'] = …` on a missing key makes a new node.
                    None => styles.push((
                        k.clone(),
                        Value::Array(vec![(Key::Str("selector".into()), root_selector)]),
                    )),
                }
            }
        }
        let mut stylesheet = Vec::new();
        if types.contains(&StylesheetType::Variables) {
            stylesheet.extend(self.css_variables(&settings, origins));
        }
        if types.contains(&StylesheetType::Styles) {
            if let Some(k) = &root_style_key
                && get_opt(options, &["skip_root_layout_styles"]).is_none_or(empty)
            {
                let base_layout_styles =
                    get_opt(options, &["base_layout_styles"]).is_some_and(|v| !empty(v));
                let root = styles
                    .iter()
                    .find(|(key, _)| key == k)
                    .map_or(Value::Null, |(_, n)| n.clone());
                stylesheet.extend(self.root_layout_rules(&root, base_layout_styles));
            }
            let nodes: Vec<Value> = styles.into_iter().map(|(_, n)| n).collect();
            stylesheet.extend(self.block_classes(&nodes));
        }
        if types.contains(&StylesheetType::Presets) {
            stylesheet.extend(self.preset_classes(&settings, origins));
        }
        // The global styles root CSS, when it is a string (WordPress 7.2).
        if types.contains(&StylesheetType::CustomCss)
            && let Some(css @ (Value::Str(_) | Value::Bytes(_))) =
                array_path::get(self.doc, &["styles", "css"])
        {
            stylesheet.extend(to_php_string(css));
        }
        Ok(stylesheet)
    }

    /// `get_custom_css()` (deprecated in 6.7): the document's CSS and each block's.
    pub fn custom_css(&self) -> Vec<u8> {
        self.deps.diagnostics.deprecated_function(
            "WP_Theme_JSON::get_custom_css",
            "6.7.0",
            "get_stylesheet",
        );
        let mut stylesheet = get_opt(self.doc, &["styles", "css"])
            .map(to_php_string)
            .unwrap_or_default();
        if array_path::isset(self.doc, &["styles", "blocks"]) {
            for (name, _) in
                entries(array_path::get(self.doc, &["styles", "blocks"]).unwrap_or(&Value::Null))
            {
                let name = key_str(name);
                let Some(css) =
                    get_opt(self.doc, &["styles", "blocks", &name, "css"]).filter(|c| !empty(c))
                else {
                    continue;
                };
                let selector = get_opt(&self.meta, &[&name, "selector"])
                    .map(text)
                    .unwrap_or_default();
                stylesheet.extend_from_slice(
                    process_blocks_custom_css(&css_text(css), &selector).as_bytes(),
                );
            }
        }
        stylesheet
    }
}
