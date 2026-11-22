//! The setting and style nodes (`get_setting_nodes`, `get_style_nodes`, `get_block_nodes`): each node is
//! WordPress's node array (`path`, `selector`, `name`, `selectors`, `elements`, `duotone`, `variations`, `css`,
//! `media_query`), listed in WordPress's order, because callbacks of `wp_theme_json_get_style_nodes` see and
//! return that list.

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::ThemeJsonError;
use speedyspec_wp_theme_json_contracts::ports::ThemeJsonFilters;
use speedyspec_wp_theme_json_contracts::schema::{
    self, ROOT_BLOCK_SELECTOR, ROOT_CSS_PROPERTIES_SELECTOR,
};

use crate::array_path::{self, child};
use crate::php_value::{as_str, entries, key_str, list, map, s};
use crate::sanitize::viewport;
use crate::selectors::append_to_selector;

/// Which nodes `get_block_nodes` lists.
#[derive(Clone, Copy, Debug, Default)]
pub struct NodeOptions {
    /// List each block's style variations with the block (`include_block_style_variations`).
    pub include_block_style_variations: bool,
    /// List paths only, without selectors (`include_node_paths_only`).
    pub include_node_paths_only: bool,
}

fn path(steps: &[&str]) -> Value {
    list(steps.iter().map(|p| s(*p)).collect())
}

fn meta_of<'a>(meta: &'a Value, name: &str, field: &str) -> Option<&'a Value> {
    array_path::get(meta, &[name, field]).filter(|v| !matches!(v, Value::Null))
}

fn or_null(value: Option<&Value>) -> Value {
    value.cloned().unwrap_or(Value::Null)
}

/// `get_setting_nodes( $theme_json, $selectors )`.
pub fn setting_nodes(doc: &Value, meta: &Value) -> Vec<Value> {
    let mut nodes = Vec::new();
    if !array_path::isset(doc, &["settings"]) {
        return nodes;
    }
    nodes.push(map(vec![
        ("path", path(&["settings"])),
        ("selector", s(ROOT_CSS_PROPERTIES_SELECTOR)),
    ]));
    if !array_path::isset(doc, &["settings", "blocks"]) {
        return nodes;
    }
    for (name, _) in entries(array_path::get(doc, &["settings", "blocks"]).unwrap_or(&Value::Null))
    {
        let name = key_str(name);
        nodes.push(map(vec![
            ("path", path(&["settings", "blocks", &name])),
            ("selector", or_null(meta_of(meta, &name, "selector"))),
            (
                "selectors",
                meta_of(meta, &name, "selectors")
                    .cloned()
                    .unwrap_or(Value::Array(Vec::new())),
            ),
        ]));
    }
    nodes
}

fn constant_list(table: &str, entry: &str) -> Vec<String> {
    let table = schema::table(table);
    entries(child(&table, &array_path::php_key(entry)).unwrap_or(&Value::Null))
        .iter()
        .filter_map(|(_, v)| as_str(v).map(|c| c.into_owned()))
        .collect()
}

/// `get_style_nodes( $theme_json, $selectors, $options )`, through `wp_theme_json_get_style_nodes` when the
/// document styles blocks (WordPress returns earlier, unfiltered, when it does not).
pub fn style_nodes(
    doc: &Value,
    meta: &Value,
    options: NodeOptions,
    filters: &dyn ThemeJsonFilters,
) -> Result<Vec<Value>, ThemeJsonError> {
    Ok(style_nodes_keyed(doc, meta, options, filters)?
        .into_iter()
        .map(|(_, v)| v)
        .collect())
}

/// `style_nodes`, keeping the array keys a `wp_theme_json_get_style_nodes` callback returned (`get_stylesheet`
/// reads the root node back by key).
pub fn style_nodes_keyed(
    doc: &Value,
    meta: &Value,
    options: NodeOptions,
    filters: &dyn ThemeJsonFilters,
) -> Result<Vec<(Key, Value)>, ThemeJsonError> {
    let keyed = |nodes: Vec<Value>| -> Vec<(Key, Value)> {
        nodes
            .into_iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v))
            .collect()
    };
    let mut nodes = Vec::new();
    if !array_path::isset(doc, &["styles"]) {
        return Ok(keyed(nodes));
    }
    nodes.push(map(vec![
        ("path", path(&["styles"])),
        ("selector", s(ROOT_BLOCK_SELECTOR)),
    ]));
    if array_path::isset(doc, &["styles", "elements"]) {
        let elements = schema::table("ELEMENTS");
        for (element, selector) in entries(&elements) {
            let element = key_str(element);
            if !array_path::isset(doc, &["styles", "elements", &element]) {
                continue;
            }
            let selector = as_str(selector).map(|c| c.into_owned()).unwrap_or_default();
            nodes.push(map(vec![
                ("path", path(&["styles", "elements", &element])),
                ("selector", s(selector.clone())),
            ]));
            for pseudo in constant_list("VALID_ELEMENT_PSEUDO_SELECTORS", &element) {
                if array_path::isset(doc, &["styles", "elements", &element, &pseudo]) {
                    nodes.push(map(vec![
                        ("path", path(&["styles", "elements", &element])),
                        ("selector", s(append_to_selector(&selector, &pseudo))),
                    ]));
                }
            }
        }
    }
    if !array_path::isset(doc, &["styles", "blocks"]) {
        return Ok(keyed(nodes));
    }
    nodes.extend(block_nodes(doc, meta, options));
    match filters.style_nodes(list(nodes))? {
        Value::Array(filtered) => Ok(filtered),
        _ => Ok(Vec::new()),
    }
}

fn append_opt(selector: &Value, pseudo: &str) -> Value {
    s(append_to_selector(
        &as_str(selector).map(|c| c.into_owned()).unwrap_or_default(),
        pseudo,
    ))
}

/// `get_block_nodes( $theme_json, $selectors, $options )`.
pub fn block_nodes(doc: &Value, meta: &Value, options: NodeOptions) -> Vec<Value> {
    let mut nodes = Vec::new();
    if !array_path::isset(doc, &["styles", "blocks"]) {
        return nodes;
    }
    let breakpoints =
        viewport::media_queries(array_path::get(doc, &["settings", "viewport"]), false);
    let blocks = array_path::get(doc, &["styles", "blocks"])
        .cloned()
        .unwrap_or(Value::Null);
    for (name_key, node) in entries(&blocks) {
        let name = key_str(name_key);
        let n = name.as_str();
        let node_path = path(&["styles", "blocks", n]);
        if options.include_node_paths_only {
            let mut variation_paths = Vec::new();
            if options.include_block_style_variations && array_path::isset(node, &["variations"]) {
                for (variation, _) in
                    entries(array_path::get(node, &["variations"]).unwrap_or(&Value::Null))
                {
                    let variation = key_str(variation);
                    variation_paths.push(map(vec![(
                        "path",
                        path(&["styles", "blocks", n, "variations", &variation]),
                    )]));
                }
            }
            let mut entry = vec![(Key::Str("path".into()), node_path.clone())];
            if !variation_paths.is_empty() {
                entry.push((Key::Str("variations".into()), list(variation_paths)));
            }
            nodes.push(Value::Array(entry));
        } else {
            let selector = or_null(meta_of(meta, n, "selector"));
            let duotone = or_null(meta_of(meta, n, "duotone"));
            let feature_selectors = or_null(meta_of(meta, n, "selectors"));
            let elements = meta_of(meta, n, "elements")
                .cloned()
                .unwrap_or(Value::Array(Vec::new()));
            let mut variations = Vec::new();
            if options.include_block_style_variations && array_path::isset(node, &["variations"]) {
                for (variation, _) in
                    entries(array_path::get(node, &["variations"]).unwrap_or(&Value::Null))
                {
                    let variation = key_str(variation);
                    variations.push(map(vec![
                        ("name", s(variation.clone())),
                        (
                            "path",
                            path(&["styles", "blocks", n, "variations", &variation]),
                        ),
                        (
                            "selector",
                            or_null(array_path::get(meta, &[n, "styleVariations", &variation])),
                        ),
                    ]));
                }
            }
            let variations = list(variations);
            nodes.push(map(vec![
                ("name", s(n)),
                ("path", node_path.clone()),
                ("selector", selector.clone()),
                ("selectors", feature_selectors.clone()),
                ("elements", elements.clone()),
                ("duotone", duotone.clone()),
                ("variations", variations.clone()),
                ("css", selector.clone()),
            ]));
            for (breakpoint, query) in &breakpoints {
                if array_path::isset(doc, &["styles", "blocks", n, breakpoint]) {
                    nodes.push(map(vec![
                        ("name", s(n)),
                        ("path", path(&["styles", "blocks", n, breakpoint])),
                        ("media_query", s(query.clone())),
                        ("selector", selector.clone()),
                        ("selectors", feature_selectors.clone()),
                        ("elements", elements.clone()),
                        ("variations", variations.clone()),
                        ("css", selector.clone()),
                    ]));
                }
            }
            for pseudo in constant_list("VALID_BLOCK_PSEUDO_SELECTORS", n) {
                let has_pseudo = array_path::isset(doc, &["styles", "blocks", n, &pseudo]);
                let has_responsive = breakpoints
                    .iter()
                    .any(|(b, _)| array_path::isset(doc, &["styles", "blocks", n, b, &pseudo]));
                if !has_pseudo && !has_responsive {
                    continue;
                }
                let pseudo_features = Value::Array(
                    entries(&feature_selectors)
                        .iter()
                        .map(|(feature, feature_selector)| {
                            let value = match feature_selector {
                                Value::Array(subs) => Value::Array(
                                    subs.iter()
                                        .map(|(sub, sel)| (sub.clone(), append_opt(sel, &pseudo)))
                                        .collect(),
                                ),
                                other => append_opt(other, &pseudo),
                            };
                            (feature.clone(), value)
                        })
                        .collect(),
                );
                let pseudo_selector = append_opt(&selector, &pseudo);
                if has_pseudo {
                    nodes.push(map(vec![
                        ("name", s(n)),
                        ("path", path(&["styles", "blocks", n, &pseudo])),
                        ("selector", pseudo_selector.clone()),
                        ("selectors", pseudo_features.clone()),
                        ("elements", elements.clone()),
                        ("duotone", duotone.clone()),
                        ("variations", variations.clone()),
                        ("css", pseudo_selector.clone()),
                    ]));
                }
                for (breakpoint, query) in &breakpoints {
                    if array_path::isset(doc, &["styles", "blocks", n, breakpoint, &pseudo]) {
                        nodes.push(map(vec![
                            ("name", s(n)),
                            ("path", path(&["styles", "blocks", n, breakpoint, &pseudo])),
                            ("media_query", s(query.clone())),
                            ("selector", pseudo_selector.clone()),
                            ("selectors", pseudo_features.clone()),
                            ("elements", elements.clone()),
                            ("variations", variations.clone()),
                            ("css", pseudo_selector.clone()),
                        ]));
                    }
                }
            }
            for state in constant_list("VALID_BLOCK_CUSTOM_STATES", n) {
                if !array_path::isset(doc, &["styles", "blocks", n, &state]) {
                    continue;
                }
                let Some(state_selector) = array_path::get(meta, &[n, "states", &state])
                    .filter(|v| !matches!(v, Value::Null))
                else {
                    continue;
                };
                let state_selector = state_selector.clone();
                nodes.push(map(vec![
                    ("name", s(n)),
                    ("path", path(&["styles", "blocks", n, &state])),
                    ("selector", state_selector.clone()),
                    ("selectors", feature_selectors.clone()),
                    ("elements", elements.clone()),
                    ("duotone", duotone.clone()),
                    ("variations", variations.clone()),
                    ("css", state_selector.clone()),
                ]));
                for pseudo in constant_list("VALID_BLOCK_PSEUDO_SELECTORS", n) {
                    if array_path::isset(doc, &["styles", "blocks", n, &state, &pseudo]) {
                        let compound = append_opt(&state_selector, &pseudo);
                        nodes.push(map(vec![
                            ("name", s(n)),
                            ("path", path(&["styles", "blocks", n, &state, &pseudo])),
                            ("selector", compound.clone()),
                            ("selectors", feature_selectors.clone()),
                            ("elements", elements.clone()),
                            ("duotone", duotone.clone()),
                            ("variations", variations.clone()),
                            ("css", compound),
                        ]));
                    }
                }
            }
        }
        let block_node = array_path::get(doc, &["styles", "blocks", n])
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let mut element_names: Vec<String> = Vec::new();
        let mut add_names = |value: Option<&Value>| {
            for (k, _) in entries(value.unwrap_or(&Value::Null)) {
                let k = key_str(k);
                if !element_names.contains(&k) {
                    element_names.push(k);
                }
            }
        };
        add_names(array_path::get(&block_node, &["elements"]));
        for (breakpoint, _) in &breakpoints {
            add_names(array_path::get(&block_node, &[breakpoint, "elements"]));
        }
        for element in &element_names {
            let e = element.as_str();
            let element_path = path(&["styles", "blocks", n, "elements", e]);
            if options.include_node_paths_only {
                if array_path::isset(&block_node, &["elements", e]) {
                    nodes.push(map(vec![("path", element_path)]));
                }
                continue;
            }
            let Some(element_selector) =
                array_path::get(meta, &[n, "elements", e]).filter(|v| !matches!(v, Value::Null))
            else {
                continue;
            };
            let element_selector = element_selector.clone();
            if array_path::isset(&block_node, &["elements", e]) {
                nodes.push(map(vec![
                    ("path", element_path.clone()),
                    ("selector", element_selector.clone()),
                ]));
            }
            for (breakpoint, query) in &breakpoints {
                if array_path::isset(doc, &["styles", "blocks", n, breakpoint, "elements", e]) {
                    nodes.push(map(vec![
                        (
                            "path",
                            path(&["styles", "blocks", n, breakpoint, "elements", e]),
                        ),
                        ("selector", element_selector.clone()),
                        ("media_query", s(query.clone())),
                    ]));
                }
            }
            for pseudo in constant_list("VALID_ELEMENT_PSEUDO_SELECTORS", e) {
                if array_path::isset(doc, &["styles", "blocks", n, "elements", e, &pseudo]) {
                    nodes.push(map(vec![
                        ("path", element_path.clone()),
                        ("selector", append_opt(&element_selector, &pseudo)),
                    ]));
                }
                for (breakpoint, query) in &breakpoints {
                    if array_path::isset(
                        doc,
                        &["styles", "blocks", n, breakpoint, "elements", e, &pseudo],
                    ) {
                        nodes.push(map(vec![
                            (
                                "path",
                                path(&["styles", "blocks", n, breakpoint, "elements", e]),
                            ),
                            ("selector", append_opt(&element_selector, &pseudo)),
                            ("media_query", s(query.clone())),
                        ]));
                    }
                }
            }
        }
    }
    nodes
}
