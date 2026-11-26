//! `get_styles_for_block( $block_metadata )` and `process_pseudo_selectors`.

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::schema::{self, ROOT_BLOCK_SELECTOR};

use super::custom_css::{css_text, process_blocks_custom_css};
use super::states::{state_base_styles, with_text_clip_escapes};
use crate::array_path;
use crate::css::{Declaration, to_ruleset};
use crate::declarations::{
    FeatureDeclarations, compute_style_properties, feature_declarations_for_node,
    update_button_width_declarations, update_paragraph_text_indent_selector,
    update_separator_declarations,
};
use crate::layout::LayoutOptions;
use crate::php::to_php_string;
use crate::php_value::{as_str, empty, entries, key_str};
use crate::sanitize::viewport;
use crate::selectors::{append_to_selector, block_style_variation_feature_selector};
use crate::sheet::Sheet;

fn text(value: &Value) -> String {
    String::from_utf8_lossy(&to_php_string(value)).into_owned()
}

fn get_opt<'v>(value: &'v Value, path: &[&str]) -> Option<&'v Value> {
    array_path::get(value, path).filter(|v| !matches!(v, Value::Null))
}

fn path_of(metadata: &Value, field: &str) -> Vec<String> {
    entries(array_path::get(metadata, &[field]).unwrap_or(&Value::Null))
        .iter()
        .map(|(_, v)| text(v))
        .collect()
}

/// `get_block_style_variation_feature_selector( $style_variation, $feature_selector )`.
fn variation_feature_selector(variation: &Value, feature_selector: &str) -> String {
    let path = path_of(variation, "path");
    let name = get_opt(variation, &["name"])
        .map(text)
        .or_else(|| path.last().cloned());
    let selector = get_opt(variation, &["selector"]).map(text);
    block_style_variation_feature_selector(name.as_deref(), selector.as_deref(), feature_selector)
}

/// `$declarations[ $selector ] = …` with PHP's in-place replacement.
fn assign(map: &mut FeatureDeclarations, selector: String, declarations: Vec<Declaration>) {
    match map.iter_mut().find(|(s, _)| *s == selector) {
        Some(slot) => slot.1 = declarations,
        None => map.push((selector, declarations)),
    }
}

fn wrap_media(media: &str, inner: &[u8]) -> Vec<u8> {
    let mut out = media.as_bytes().to_vec();
    out.push(b'{');
    out.extend_from_slice(inner);
    out.push(b'}');
    out
}

/// `get_pseudo_rule_selector( $base_selector, $pseudo_selector )`: a pseudo-class goes on each part inside
/// `:root :where()`; a pseudo-element, not valid inside `:where()`, goes after it (WordPress 7.2).
fn pseudo_rule_selector(base_selector: &str, pseudo_selector: &str) -> String {
    if pseudo_selector.starts_with("::") {
        return format!(":root :where({base_selector}){pseudo_selector}");
    }
    format!(
        ":root :where({})",
        append_to_selector(base_selector, pseudo_selector)
    )
}

fn element_pseudo_selectors(element: &str) -> Vec<String> {
    let table = schema::table("VALID_ELEMENT_PSEUDO_SELECTORS");
    entries(array_path::get(&table, &[element]).unwrap_or(&Value::Null))
        .iter()
        .filter_map(|(_, v)| as_str(v).map(|c| c.into_owned()))
        .collect()
}

fn block_pseudo_selectors(block: &str) -> Option<Vec<String>> {
    let table = schema::table("VALID_BLOCK_PSEUDO_SELECTORS");
    array_path::get(&table, &[block])
        .filter(|v| !matches!(v, Value::Null))
        .map(|list| {
            entries(list)
                .iter()
                .filter_map(|(_, v)| as_str(v).map(|c| c.into_owned()))
                .collect()
        })
}

impl Sheet<'_> {
    fn node_at(&self, path: &[String]) -> Value {
        let refs: Vec<&str> = path.iter().map(String::as_str).collect();
        array_path::get(self.doc, &refs)
            .cloned()
            .unwrap_or(Value::Array(Vec::new()))
    }

    fn adjusted_features(
        &self,
        metadata: &Value,
        node: &mut Value,
        settings: &Value,
        block_name: Option<&str>,
    ) -> FeatureDeclarations {
        let d = feature_declarations_for_node(metadata, node, settings, self.doc, self.decl());
        let d = update_paragraph_text_indent_selector(d, settings, block_name);
        update_button_width_declarations(d, settings)
    }

    /// `process_pseudo_selectors( $node, $base_selector, $settings, $block_name, $block_metadata, $style_variation )`.
    pub fn process_pseudo_selectors(
        &self,
        node: &Value,
        base_selector: &str,
        settings: &Value,
        block_name: Option<&str>,
        metadata: Option<&Value>,
        variation: Option<&Value>,
    ) -> FeatureDeclarations {
        let mut result = FeatureDeclarations::new();
        let mut add = |selector: String, declarations: Vec<Declaration>| {
            if declarations.is_empty() {
                return;
            }
            match result.iter_mut().find(|(s, _)| *s == selector) {
                Some(slot) => slot.1.extend(declarations),
                None => result.push((selector, declarations)),
            }
        };
        let Some(pseudos) = block_name.and_then(block_pseudo_selectors) else {
            return result;
        };
        // Nearest first: this node, then its variation's, then its block's.
        let mut base_styles = vec![node.clone()];
        for owner in [variation, metadata].into_iter().flatten() {
            let layer_path = path_of(owner, "path");
            if layer_path.is_empty() {
                continue;
            }
            let refs: Vec<&str> = layer_path.iter().map(String::as_str).collect();
            if let Some(layer @ Value::Array(_)) = array_path::get(self.doc, &refs) {
                base_styles.push(layer.clone());
            }
        }
        for pseudo in pseudos {
            let Some(pseudo_node) = get_opt(node, &[&pseudo]) else {
                continue;
            };
            let mut pseudo_node = pseudo_node.clone();
            if let Some(metadata @ Value::Array(_)) = metadata {
                for (feature_selector, declarations) in
                    self.adjusted_features(metadata, &mut pseudo_node, settings, block_name)
                {
                    let target = match variation {
                        Some(v @ Value::Array(_)) => {
                            variation_feature_selector(v, &feature_selector)
                        }
                        _ => feature_selector,
                    };
                    add(append_to_selector(&target, &pseudo), declarations);
                }
            }
            let declarations = compute_style_properties(
                &pseudo_node,
                settings,
                None,
                None,
                None,
                false,
                self.decl(),
            );
            let declarations = with_text_clip_escapes(declarations, &pseudo_node, &base_styles);
            add(append_to_selector(base_selector, &pseudo), declarations);
        }
        result
    }

    /// `get_styles_for_block( $block_metadata )`: the CSS for one style node.
    pub fn styles_for_block(&self, metadata: &Value) -> Vec<u8> {
        let path = path_of(metadata, "path");
        let mut node = self.node_at(&path);
        let use_root_padding = self.use_root_padding();
        let selector = get_opt(metadata, &["selector"])
            .map(text)
            .unwrap_or_default();
        let settings = self.settings();
        let mut block_name: Option<String> = get_opt(metadata, &["name"]).map(text);
        let mut feature_declarations =
            feature_declarations_for_node(metadata, &mut node, &settings, self.doc, self.decl());
        let is_root = selector == ROOT_BLOCK_SELECTOR;
        let media_query = get_opt(metadata, &["media_query"]).map(text);
        let breakpoints = viewport::media_queries(array_path::get(&settings, &["viewport"]), false);
        feature_declarations = update_paragraph_text_indent_selector(
            feature_declarations,
            &settings,
            block_name.as_deref(),
        );
        let block_elements = get_opt(metadata, &["elements"])
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        feature_declarations = update_button_width_declarations(feature_declarations, &settings);
        let metadata_css = get_opt(metadata, &["css"]).map(text).unwrap_or_default();
        // The block's `css` feature selector, which custom CSS targets.
        let css_feature_selector = match get_opt(metadata, &["selectors", "css"]) {
            Some(Value::Array(_)) => get_opt(metadata, &["selectors", "css", "root"]),
            other => other,
        }
        .and_then(|v| match v {
            Value::Str(s) => Some(s.clone()),
            _ => None,
        });

        let mut variation_declarations = FeatureDeclarations::new();
        let mut variation_custom_css: Vec<(String, String)> = Vec::new();
        let mut variation_responsive_css: Vec<(String, Vec<u8>)> = Vec::new();
        let mut variation_responsive_pseudo_css: Vec<(String, Vec<u8>)> = Vec::new();
        let mut variation_layout: Vec<(String, Value, Value)> = Vec::new();
        let variations = get_opt(metadata, &["variations"])
            .cloned()
            .unwrap_or(Value::Null);
        let has_media_query = media_query
            .as_ref()
            .is_some_and(|m| !m.is_empty() && m != "0");
        if !has_media_query && !empty(&variations) {
            for (_, variation) in entries(&variations) {
                let variation_selector = get_opt(variation, &["selector"])
                    .map(text)
                    .unwrap_or_default();
                let mut variation_node = self.node_at(&path_of(variation, "path"));
                let features = self.adjusted_features(
                    metadata,
                    &mut variation_node,
                    &settings,
                    block_name.as_deref(),
                );
                for (current, declarations) in features {
                    assign(
                        &mut variation_declarations,
                        variation_feature_selector(variation, &current),
                        declarations,
                    );
                }
                let own = compute_style_properties(
                    &variation_node,
                    &settings,
                    None,
                    Some(self.doc),
                    None,
                    false,
                    self.decl(),
                );
                assign(&mut variation_declarations, variation_selector.clone(), own);
                block_name = if let Some(name) = get_opt(metadata, &["name"]) {
                    Some(text(name))
                } else if path.iter().any(|p| p == "blocks") && path.len() >= 3 {
                    Some(path[2].clone())
                } else {
                    None
                };
                for (s, d) in self.process_pseudo_selectors(
                    &variation_node,
                    &variation_selector,
                    &settings,
                    block_name.as_deref(),
                    Some(metadata),
                    Some(variation),
                ) {
                    assign(&mut variation_declarations, s, d);
                }
                let variation_css_selector = match &css_feature_selector {
                    Some(feature) => variation_feature_selector(variation, feature),
                    None => variation_selector.clone(),
                };
                if let Some(css) = get_opt(&variation_node, &["css"]) {
                    let processed =
                        process_blocks_custom_css(&css_text(css), &variation_css_selector);
                    match variation_custom_css
                        .iter_mut()
                        .find(|(s, _)| *s == variation_selector)
                    {
                        Some(slot) => slot.1 = processed,
                        None => variation_custom_css.push((variation_selector.clone(), processed)),
                    }
                }
                if array_path::isset(&variation_node, &["spacing", "blockGap"]) {
                    let mut layout_metadata = variation.clone();
                    array_path::insert(
                        &mut layout_metadata,
                        Key::Str("selector".into()),
                        Value::Str(format!("{variation_selector}{metadata_css}")),
                    );
                    array_path::insert(
                        &mut layout_metadata,
                        Key::Str("name".into()),
                        block_name.clone().map_or(Value::Null, Value::Str),
                    );
                    match variation_layout
                        .iter_mut()
                        .find(|(s, _, _)| *s == variation_selector)
                    {
                        Some(slot) => {
                            *slot = (
                                variation_selector.clone(),
                                layout_metadata,
                                variation_node.clone(),
                            )
                        }
                        None => variation_layout.push((
                            variation_selector.clone(),
                            layout_metadata,
                            variation_node.clone(),
                        )),
                    }
                }
                let mut responsive_css = Vec::new();
                let mut responsive_pseudo_css = Vec::new();
                for (breakpoint, media) in &breakpoints {
                    let Some(breakpoint_node) = get_opt(&variation_node, &[breakpoint]) else {
                        continue;
                    };
                    let mut breakpoint_node = breakpoint_node.clone();
                    for (feature_selector, declarations) in self.adjusted_features(
                        metadata,
                        &mut breakpoint_node,
                        &settings,
                        block_name.as_deref(),
                    ) {
                        let combined = variation_feature_selector(variation, &feature_selector);
                        responsive_css.extend(wrap_media(
                            media,
                            &to_ruleset(&format!(":root :where({combined})"), &declarations),
                        ));
                    }
                    let base = compute_style_properties(
                        &breakpoint_node,
                        &settings,
                        None,
                        Some(self.doc),
                        None,
                        false,
                        self.decl(),
                    );
                    let base = with_text_clip_escapes(
                        base,
                        &breakpoint_node,
                        &[variation_node.clone(), node.clone()],
                    );
                    if !base.is_empty() {
                        responsive_css.extend(wrap_media(
                            media,
                            &to_ruleset(&format!(":root :where({variation_selector})"), &base),
                        ));
                    }
                    for (pseudo_selector, declarations) in self.process_pseudo_selectors(
                        &breakpoint_node,
                        &variation_selector,
                        &settings,
                        block_name.as_deref(),
                        Some(metadata),
                        Some(variation),
                    ) {
                        if declarations.is_empty() {
                            continue;
                        }
                        responsive_pseudo_css.extend(wrap_media(
                            media,
                            &to_ruleset(&format!(":root :where({pseudo_selector})"), &declarations),
                        ));
                    }
                    if let Some(css) = get_opt(&breakpoint_node, &["css"]) {
                        responsive_css.extend(wrap_media(
                            media,
                            process_blocks_custom_css(&css_text(css), &variation_css_selector)
                                .as_bytes(),
                        ));
                    }
                    if array_path::isset(&breakpoint_node, &["spacing", "blockGap"]) {
                        let mut layout_metadata = variation.clone();
                        array_path::insert(
                            &mut layout_metadata,
                            Key::Str("selector".into()),
                            Value::Str(format!("{variation_selector}{metadata_css}")),
                        );
                        array_path::insert(
                            &mut layout_metadata,
                            Key::Str("name".into()),
                            block_name.clone().map_or(Value::Null, Value::Str),
                        );
                        responsive_css.extend(self.layout_styles(
                            &layout_metadata,
                            &LayoutOptions {
                                node: Some(breakpoint_node.clone()),
                                media_query: Some(media.clone()),
                                base_layout_styles: false,
                            },
                        ));
                    }
                    if let Some(elements) = get_opt(&breakpoint_node, &["elements"])
                        && !empty(&block_elements)
                    {
                        for (element_name, element_node) in entries(elements) {
                            let element_name = key_str(element_name);
                            let Some(element_selector) = get_opt(&block_elements, &[&element_name])
                            else {
                                continue;
                            };
                            let element_selector =
                                variation_feature_selector(variation, &text(element_selector));
                            let declarations = compute_style_properties(
                                element_node,
                                &settings,
                                None,
                                Some(self.doc),
                                None,
                                false,
                                self.decl(),
                            );
                            if !declarations.is_empty() {
                                responsive_css.extend(wrap_media(
                                    media,
                                    &to_ruleset(
                                        &format!(":root :where({element_selector})"),
                                        &declarations,
                                    ),
                                ));
                            }
                            if let Some(css) = get_opt(element_node, &["css"]) {
                                responsive_css.extend(wrap_media(
                                    media,
                                    process_blocks_custom_css(&css_text(css), &element_selector)
                                        .as_bytes(),
                                ));
                            }
                            for pseudo in element_pseudo_selectors(&element_name) {
                                let Some(pseudo_node) = get_opt(element_node, &[&pseudo]) else {
                                    continue;
                                };
                                let declarations = compute_style_properties(
                                    pseudo_node,
                                    &settings,
                                    None,
                                    Some(self.doc),
                                    None,
                                    false,
                                    self.decl(),
                                );
                                if declarations.is_empty() {
                                    continue;
                                }
                                responsive_pseudo_css.extend(wrap_media(
                                    media,
                                    &to_ruleset(
                                        &pseudo_rule_selector(&element_selector, &pseudo),
                                        &declarations,
                                    ),
                                ));
                            }
                        }
                    }
                }
                if !responsive_css.is_empty() {
                    match variation_responsive_css
                        .iter_mut()
                        .find(|(s, _)| *s == variation_selector)
                    {
                        Some(slot) => slot.1 = responsive_css,
                        None => variation_responsive_css
                            .push((variation_selector.clone(), responsive_css)),
                    }
                }
                if !responsive_pseudo_css.is_empty() {
                    match variation_responsive_pseudo_css
                        .iter_mut()
                        .find(|(s, _)| *s == variation_selector)
                    {
                        Some(slot) => slot.1 = responsive_pseudo_css,
                        None => variation_responsive_pseudo_css
                            .push((variation_selector.clone(), responsive_pseudo_css)),
                    }
                }
            }
        }

        let is_processing_element = path.iter().any(|p| p == "elements");
        let current_element = if is_processing_element {
            path.last().cloned()
        } else {
            None
        };
        let allowed = current_element
            .as_deref()
            .map(element_pseudo_selectors)
            .unwrap_or_default();
        let pseudo_selector = allowed.iter().find(|p| {
            let mut from = 0;
            while let Some(i) = selector[from..].find(p.as_str()) {
                let end = from + i + p.len();
                if !selector[end..].starts_with('-') {
                    return true;
                }
                from += i + 1;
            }
            false
        });
        let base_styles = state_base_styles(&path, self.doc, &settings);
        let mut declarations =
            match pseudo_selector.and_then(|p| get_opt(&node, &[p.as_str()]).map(|n| (p, n))) {
                Some((_, pseudo_node)) => {
                    let declarations = compute_style_properties(
                        pseudo_node,
                        &settings,
                        None,
                        Some(self.doc),
                        Some(&selector),
                        use_root_padding,
                        self.decl(),
                    );
                    let mut layers = vec![node.clone()];
                    layers.extend(base_styles.iter().cloned());
                    with_text_clip_escapes(declarations, pseudo_node, &layers)
                }
                None => {
                    let declarations = compute_style_properties(
                        &node,
                        &settings,
                        None,
                        Some(self.doc),
                        Some(&selector),
                        use_root_padding,
                        self.decl(),
                    );
                    with_text_clip_escapes(declarations, &node, &base_styles)
                }
            };

        let mut block_rules = Vec::new();
        let mut duotone = Vec::new();
        let mut set_root_min_height = false;
        let mut kept = Vec::with_capacity(declarations.len());
        for declaration in declarations.drain(..) {
            let is_filter = declaration.name == "filter";
            if is_root
                && (declaration.name == "background-image" || declaration.name == "background")
            {
                set_root_min_height = true;
            }
            if is_filter {
                if !matches!(&declaration.value, Value::Str(s) if s == "unset") {
                    duotone.push(declaration);
                }
                continue;
            }
            kept.push(declaration);
        }
        let mut declarations = kept;
        if set_root_min_height {
            block_rules.extend(to_ruleset(
                "html",
                &[Declaration::text(
                    "min-height",
                    "calc(100% - var(--wp-admin--admin-bar--height, 0px))",
                )],
            ));
        }
        if selector == ".wp-block-separator" {
            declarations = update_separator_declarations(declarations);
        }
        let element_only = is_root || current_element.as_deref().is_some_and(|element| {
            let elements = schema::table("ELEMENTS");
            let class_names = schema::table("__EXPERIMENTAL_ELEMENT_CLASS_NAMES");
            matches!(array_path::get(&elements, &[element]), Some(Value::Str(s)) if *s == selector)
                && !array_path::isset(&class_names, &[element])
        });
        let general = if let Some(pseudo) = pseudo_selector.filter(|p| p.starts_with("::")) {
            // The selector carries the pseudo-element on each part: strip it for the base selector.
            pseudo_rule_selector(&selector.replace(pseudo.as_str(), ""), pseudo)
        } else if element_only {
            selector.clone()
        } else {
            format!(":root :where({selector})")
        };
        block_rules.extend(to_ruleset(&general, &declarations));
        if let Some(duotone_selector) = get_opt(metadata, &["duotone"])
            && !duotone.is_empty()
        {
            block_rules.extend(to_ruleset(&text(duotone_selector), &duotone));
        }
        if !is_root && get_opt(metadata, &["name"]).is_some_and(|n| !empty(n)) {
            block_rules.extend(self.layout_styles(metadata, &LayoutOptions::default()));
        }
        for (feature_selector, declarations) in &feature_declarations {
            block_rules.extend(to_ruleset(
                &format!(":root :where({feature_selector})"),
                declarations,
            ));
        }
        for (variation_selector, declarations) in &variation_declarations {
            block_rules.extend(to_ruleset(
                &format!(":root :where({variation_selector})"),
                declarations,
            ));
            if let Some((_, layout_metadata, layout_node)) = variation_layout
                .iter()
                .find(|(s, _, _)| s == variation_selector)
            {
                block_rules.extend(self.layout_styles(
                    layout_metadata,
                    &LayoutOptions {
                        node: Some(layout_node.clone()),
                        ..LayoutOptions::default()
                    },
                ));
            }
            if let Some((_, css)) = variation_custom_css
                .iter()
                .find(|(s, _)| s == variation_selector)
            {
                block_rules.extend_from_slice(css.as_bytes());
            }
            if let Some((_, css)) = variation_responsive_css
                .iter()
                .find(|(s, _)| s == variation_selector)
            {
                block_rules.extend_from_slice(css);
            }
        }
        for (_, css) in &variation_responsive_pseudo_css {
            block_rules.extend_from_slice(css);
        }
        if let Some(css) = get_opt(&node, &["css"])
            && !is_root
        {
            let css_selector = css_feature_selector
                .clone()
                .unwrap_or_else(|| selector.clone());
            block_rules.extend_from_slice(
                process_blocks_custom_css(&css_text(css), &css_selector).as_bytes(),
            );
        }
        if let Some(media) = media_query.filter(|m| !m.is_empty() && m != "0")
            && !block_rules.is_empty()
        {
            return wrap_media(&media, &block_rules);
        }
        block_rules
    }
}
