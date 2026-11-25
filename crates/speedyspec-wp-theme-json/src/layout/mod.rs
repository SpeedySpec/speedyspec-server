//! Layout styles: `get_layout_styles` and `get_root_layout_rules`, driven by WordPress's layout definitions
//! (`wp_get_layout_definitions()`, exported from the release).

use speedyspec_hooks::Value;
use speedyspec_wp_theme_json_contracts::ports::RegisteredBlock;
use speedyspec_wp_theme_json_contracts::schema::{
    ROOT_BLOCK_SELECTOR, ROOT_CSS_PROPERTIES_SELECTOR, layout_definitions,
};

use crate::array_path;
use crate::css::{Declaration, to_ruleset};
use crate::declarations::property_value;
use crate::php::to_php_string;
use crate::php_value::{as_str, empty, entries, key_str};
use crate::sheet::Sheet;

/// `get_layout_styles`' `$options`.
#[derive(Clone, Debug, Default)]
pub struct LayoutOptions {
    /// The node to read the block gap from, instead of the metadata's path.
    pub node: Option<Value>,
    /// A media query to wrap the rules in.
    pub media_query: Option<String>,
    /// Leave out the flow and constrained base styles (`base_layout_styles`).
    pub base_layout_styles: bool,
}

/// `block_has_support( $block_type, $feature )` for a top-level feature.
fn has_support(block: Option<&RegisteredBlock>, feature: &str) -> bool {
    let Some(block) = block else { return false };
    matches!(
        array_path::get(&block.supports, &[feature]),
        Some(Value::Bool(true)) | Some(Value::Array(_))
    )
}

/// `preg_match( '/^[a-zA-Z0-9\-\.\,\ *+>:\(\)]*$/', $selector )`.
fn layout_selector_ok(selector: &str) -> bool {
    let body = selector.strip_suffix('\n').unwrap_or(selector);
    body.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-., *+>:()".contains(&b))
}

fn text(value: &Value) -> String {
    String::from_utf8_lossy(&to_php_string(value)).into_owned()
}

impl Sheet<'_> {
    /// `get_layout_styles( $block_metadata, $options )`.
    pub fn layout_styles(&self, metadata: &Value, options: &LayoutOptions) -> Vec<u8> {
        let mut rules = Vec::new();
        if self.deps.theme.supports("disable-layout-styles") {
            return rules;
        }
        let current;
        let mut block_type = None;
        if let Some(name) =
            array_path::get(metadata, &["name"]).filter(|n| !matches!(n, Value::Null))
        {
            // WordPress reads the live registry here, not its remembered metadata.
            current = self.deps.blocks.current(&text(name));
            block_type = current.as_ref();
            if !has_support(block_type, "layout")
                && !has_support(block_type, "__experimentalLayout")
            {
                return rules;
            }
        }
        let selector = array_path::get(metadata, &["selector"])
            .filter(|v| !matches!(v, Value::Null))
            .map(text)
            .unwrap_or_default();
        let is_root = selector == ROOT_BLOCK_SELECTOR;
        let has_block_gap_support =
            array_path::isset(self.doc, &["settings", "spacing", "blockGap"]);
        let node = match (&options.node, array_path::get(metadata, &["path"])) {
            (Some(n), _) => n.clone(),
            (None, Some(path @ Value::Array(_))) => {
                let path: Vec<String> = entries(path).iter().map(|(_, v)| text(v)).collect();
                let refs: Vec<&str> = path.iter().map(String::as_str).collect();
                array_path::get(self.doc, &refs)
                    .cloned()
                    .unwrap_or(Value::Array(Vec::new()))
            }
            // `_wp_array_get()` with no path gives its default.
            (None, _) => Value::Array(Vec::new()),
        };
        let definitions = layout_definitions();

        let mut gap = if !has_block_gap_support {
            let mut gap = if is_root {
                Value::Str("0.5em".into())
            } else {
                Value::Null
            };
            if let Some(block) = block_type {
                gap = array_path::get(
                    &block.supports,
                    &["spacing", "blockGap", "__experimentalDefault"],
                )
                .cloned()
                .unwrap_or(Value::Null);
            }
            gap
        } else {
            property_value(
                &node,
                &["spacing".into(), "blockGap".into()],
                None,
                self.decl(),
            )
        };
        // The row value alone sets the flow and constrained layouts' vertical margins (WordPress 7.2); a missing
        // axis of a split value is `0`.
        let mut row_gap = gap.clone();
        if let Value::Array(_) = gap {
            let has_row = array_path::isset(&gap, &["top"]);
            let has_column = array_path::isset(&gap, &["left"]);
            if has_row || has_column {
                let axis = |name: &str, present: bool| {
                    if present {
                        property_value(
                            &node,
                            &["spacing".into(), "blockGap".into(), name.into()],
                            None,
                            self.decl(),
                        )
                    } else {
                        Value::Str("0".into())
                    }
                };
                let row = axis("top", has_row);
                let column = axis("left", has_column);
                gap = if row == column {
                    row.clone()
                } else {
                    Value::Str(format!("{} {}", text(&row), text(&column)))
                };
                row_gap = row;
            } else {
                gap = Value::Null;
                row_gap = Value::Null;
            }
        }
        let has_gap = !matches!(gap, Value::Null | Value::Bool(false))
            && !matches!(&gap, Value::Str(s) if s.is_empty());
        if has_gap {
            for (key, definition) in entries(&definitions) {
                let key = key_str(key);
                if !has_block_gap_support && key != "flex" && key != "grid" {
                    continue;
                }
                let class_name = array_path::get(definition, &["className"])
                    .cloned()
                    .unwrap_or(Value::Bool(false));
                let spacing_rules = array_path::get(definition, &["spacingStyles"])
                    .cloned()
                    .unwrap_or(Value::Null);
                if empty(&class_name) || empty(&spacing_rules) {
                    continue;
                }
                let class_name = text(&class_name);
                let layout_gap = if key == "default" || key == "constrained" {
                    &row_gap
                } else {
                    &gap
                };
                for (_, rule) in entries(&spacing_rules) {
                    let Some(rule_selector) =
                        array_path::get(rule, &["selector"]).filter(|v| !matches!(v, Value::Null))
                    else {
                        continue;
                    };
                    let rule_selector = text(rule_selector);
                    let css_rules = array_path::get(rule, &["rules"])
                        .cloned()
                        .unwrap_or(Value::Null);
                    if !layout_selector_ok(&rule_selector) || empty(&css_rules) {
                        continue;
                    }
                    let mut declarations = Vec::new();
                    for (property, value) in entries(&css_rules) {
                        let property = key_str(property);
                        let current = if as_str(value).is_some() {
                            value.clone()
                        } else {
                            layout_gap.clone()
                        };
                        if self.is_safe_css_declaration(&property, &current) {
                            declarations.push(Declaration::new(property, current));
                        }
                    }
                    let layout_selector = if !has_block_gap_support {
                        if is_root {
                            format!(":where(.{class_name}{rule_selector})")
                        } else {
                            format!(":where({selector}.{class_name}{rule_selector})")
                        }
                    } else if is_root {
                        format!(":root :where(.{class_name}){rule_selector}")
                    } else {
                        format!(":root :where({selector}-{class_name}){rule_selector}")
                    };
                    rules.extend(to_ruleset(&layout_selector, &declarations));
                }
            }
        }

        if is_root {
            let has_content_size =
                array_path::isset(self.doc, &["settings", "layout", "contentSize"])
                    || array_path::isset(self.doc, &["settings", "layout", "wideSize"]);
            for (_, definition) in entries(&definitions) {
                let class_name = array_path::get(definition, &["className"])
                    .cloned()
                    .unwrap_or(Value::Bool(false));
                let base = array_path::get(definition, &["baseStyles"])
                    .cloned()
                    .unwrap_or(Value::Array(Vec::new()));
                if empty(&class_name) || !matches!(base, Value::Array(_)) {
                    continue;
                }
                let class_name = text(&class_name);
                if let Some(Value::Str(mode)) = array_path::get(definition, &["displayMode"])
                    && ["block", "flex", "grid"].contains(&mode.as_str())
                {
                    rules.extend(to_ruleset(
                        &format!("{selector} .{class_name}"),
                        &[Declaration::text("display", mode.clone())],
                    ));
                }
                let name = array_path::get(definition, &["name"])
                    .map(text)
                    .unwrap_or_default();
                for (_, rule) in entries(&base) {
                    if options.base_layout_styles && (name == "default" || name == "constrained") {
                        continue;
                    }
                    let Some(rule_selector) =
                        array_path::get(rule, &["selector"]).filter(|v| !matches!(v, Value::Null))
                    else {
                        continue;
                    };
                    let rule_selector = text(rule_selector);
                    let css_rules = array_path::get(rule, &["rules"])
                        .cloned()
                        .unwrap_or(Value::Null);
                    if !layout_selector_ok(&rule_selector) || empty(&css_rules) {
                        continue;
                    }
                    let mut declarations = Vec::new();
                    for (property, value) in entries(&css_rules) {
                        let property = key_str(property);
                        if let Some(v) = as_str(value)
                            && (v.contains("--global--content-size")
                                || v.contains("--global--wide-size"))
                            && !has_content_size
                        {
                            continue;
                        }
                        if self.is_safe_css_declaration(&property, value) {
                            declarations.push(Declaration::new(property, value.clone()));
                        }
                    }
                    rules.extend(to_ruleset(
                        &format!(".{class_name}{rule_selector}"),
                        &declarations,
                    ));
                }
            }
        }
        if let Some(query) = options
            .media_query
            .as_ref()
            .filter(|q| !q.is_empty() && q.as_str() != "0")
            && !rules.is_empty()
        {
            let mut wrapped = query.as_bytes().to_vec();
            wrapped.push(b'{');
            wrapped.extend(rules);
            wrapped.push(b'}');
            return wrapped;
        }
        rules
    }

    /// `get_root_layout_rules( $selector, $block_metadata, $options )`.
    pub fn root_layout_rules(&self, metadata: &Value, base_layout_styles: bool) -> Vec<u8> {
        let mut css = String::new();
        let settings = self.settings();
        let use_root_padding = self.use_root_padding();
        let content = array_path::get(&settings, &["layout", "contentSize"])
            .filter(|v| !matches!(v, Value::Null));
        let wide = array_path::get(&settings, &["layout", "wideSize"])
            .filter(|v| !matches!(v, Value::Null));
        if content.is_some() || wide.is_some() {
            let content_size = content.or(wide).cloned().unwrap_or(Value::Null);
            let content_size = if self.is_safe_css_declaration("max-width", &content_size) {
                text(&content_size)
            } else {
                "initial".into()
            };
            let wide_size = wide.or(content).cloned().unwrap_or(Value::Null);
            let wide_size = if self.is_safe_css_declaration("max-width", &wide_size) {
                text(&wide_size)
            } else {
                "initial".into()
            };
            css.push_str(&format!("{ROOT_CSS_PROPERTIES_SELECTOR} {{ --wp--style--global--content-size: {content_size};"));
            css.push_str(&format!("--wp--style--global--wide-size: {wide_size}; }}"));
        }
        css.push_str(":where(body) { margin: 0; }");
        if use_root_padding {
            css.push_str(".wp-site-blocks { padding-top: var(--wp--style--root--padding-top); padding-bottom: var(--wp--style--root--padding-bottom); }");
            css.push_str(".has-global-padding { padding-right: var(--wp--style--root--padding-right); padding-left: var(--wp--style--root--padding-left); }");
            css.push_str(".has-global-padding > .alignfull { margin-right: calc(var(--wp--style--root--padding-right) * -1); margin-left: calc(var(--wp--style--root--padding-left) * -1); }");
            css.push_str(".has-global-padding :where(:not(.alignfull.is-layout-flow) > .has-global-padding:not(.wp-block-block, .alignfull)) { padding-right: 0; padding-left: 0; }");
            css.push_str(".has-global-padding :where(:not(.alignfull.is-layout-flow) > .has-global-padding:not(.wp-block-block, .alignfull)) > .alignfull { margin-left: 0; margin-right: 0; }");
        }
        if !base_layout_styles {
            css.push_str(".wp-site-blocks > .alignleft { float: left; margin-right: 2em; }");
            css.push_str(".wp-site-blocks > .alignright { float: right; margin-left: 2em; }");
            css.push_str(".wp-site-blocks > .aligncenter { justify-content: center; margin-left: auto; margin-right: auto; }");
        }
        if array_path::isset(self.doc, &["settings", "spacing", "blockGap"]) {
            let gap = text(&property_value(
                self.doc,
                &["styles".into(), "spacing".into(), "blockGap".into()],
                None,
                self.decl(),
            ));
            css.push_str(&format!(
                ":where(.wp-site-blocks) > * {{ margin-block-start: {gap}; margin-block-end: 0; }}"
            ));
            css.push_str(":where(.wp-site-blocks) > :first-child { margin-block-start: 0; }");
            css.push_str(":where(.wp-site-blocks) > :last-child { margin-block-end: 0; }");
            css.push_str(&format!(
                "{ROOT_CSS_PROPERTIES_SELECTOR} {{ --wp--style--block-gap: {gap}; }}"
            ));
        }
        let mut out = css.into_bytes();
        out.extend(self.layout_styles(
            metadata,
            &LayoutOptions {
                base_layout_styles,
                ..LayoutOptions::default()
            },
        ));
        out
    }
}
