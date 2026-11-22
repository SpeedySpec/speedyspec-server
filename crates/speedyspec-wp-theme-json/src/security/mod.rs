//! Security: `remove_insecure_properties` and its helpers, for data from users who may not write arbitrary CSS
//! (Specifications over kses's `SafeCss`, the text escapers and the user's capabilities).

mod settings;
mod styles;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::ports::{SafeCss, TextFunctions};
use speedyspec_wp_theme_json_contracts::schema;
use speedyspec_wp_theme_json_contracts::{Origin, ThemeJsonError};

use crate::array_path;
use crate::construct::valid_names;
use crate::declarations::DeclarationContext;
use crate::nodes::{NodeOptions, setting_nodes, style_nodes};
use crate::php::to_php_string;
use crate::php_value::{as_str, empty, entries};
use crate::presets::FluidFontSize;
use crate::sanitize::{sanitize, viewport};
use crate::selectors::BlocksMetadataCache;
use crate::{ThemeJsonDeps, schema_migrate};

/// `is_safe_css_declaration( $property_name, $property_value )`: `esc_html( safecss_filter_attr( "name: value" ) )`
/// is not empty once trimmed.
pub fn is_safe_css_declaration(
    name: &str,
    value: &Value,
    safe_css: &dyn SafeCss,
    text: &dyn TextFunctions,
) -> bool {
    let mut style = name.as_bytes().to_vec();
    style.extend_from_slice(b": ");
    style.extend_from_slice(&to_php_string(value));
    let filtered = safe_css.safecss_filter_attr(&style).unwrap_or_default();
    let escaped = text.esc_html(&filtered);
    let is_space = |b: &u8| b" \t\n\r\0\x0b".contains(b);
    let start = escaped
        .iter()
        .position(|b| !is_space(b))
        .unwrap_or(escaped.len());
    let end = escaped
        .iter()
        .rposition(|b| !is_space(b))
        .map_or(start, |e| e + 1);
    let trimmed = &escaped[start..end.max(start)];
    !(trimmed.is_empty() || trimmed == b"0")
}

/// What removing insecure properties reads from outside.
pub(crate) struct Security<'a> {
    pub(crate) deps: &'a ThemeJsonDeps,
    pub(crate) fluid: FluidFontSize,
}

impl Security<'_> {
    pub(crate) fn decl(&self) -> DeclarationContext<'_> {
        DeclarationContext {
            fluid: &self.fluid,
            diagnostics: &*self.deps.diagnostics,
            gettext: &*self.deps.gettext,
        }
    }

    pub(crate) fn safe(&self, name: &str, value: &Value) -> bool {
        is_safe_css_declaration(name, value, &*self.deps.safe_css, &*self.deps.text)
    }

    fn can_edit_css(&self) -> bool {
        self.deps.capabilities.current_user_can("edit_css")
    }
}

fn path_of(node: &Value) -> Vec<String> {
    entries(array_path::get(node, &["path"]).unwrap_or(&Value::Null))
        .iter()
        .map(|(_, v)| String::from_utf8_lossy(&to_php_string(v)).into_owned())
        .collect()
}

fn get_path<'v>(value: &'v Value, path: &[String]) -> Option<&'v Value> {
    let refs: Vec<&str> = path.iter().map(String::as_str).collect();
    array_path::get(value, &refs)
}

fn set_path(value: &mut Value, path: &[String], new: Value) {
    let refs: Vec<&str> = path.iter().map(String::as_str).collect();
    array_path::set(value, &refs, new);
}

fn pseudo_list(table: &str, entry: &str) -> Option<Vec<String>> {
    let table = schema::table(table);
    array_path::get(&table, &[entry])
        .filter(|v| !matches!(v, Value::Null))
        .map(|list| {
            entries(list)
                .iter()
                .filter_map(|(_, v)| as_str(v).map(|c| c.into_owned()))
                .collect()
        })
}

fn put(target: &mut Value, key: &str, value: Value) {
    array_path::insert(target, array_path::php_key(key), value);
}

/// `WP_Theme_JSON::remove_insecure_properties( $theme_json, $origin )`. A failing `wp_theme_json_get_style_nodes`
/// callback is an error, as its exception is in WordPress.
pub fn remove_insecure_properties(
    doc: Value,
    origin: &Value,
    deps: &ThemeJsonDeps,
    metadata: &BlocksMetadataCache,
) -> Result<Value, ThemeJsonError> {
    let origin = match origin {
        Value::Str(s) => Origin::parse(s.as_bytes()).unwrap_or(Origin::Theme),
        _ => Origin::Theme,
    };
    let security = Security {
        deps,
        fluid: FluidFontSize {
            global: deps.global_settings.clone(),
            diagnostics: deps.diagnostics.clone(),
        },
    };
    let mut theme_json = schema_migrate::migrate(doc, origin);
    let names = valid_names(deps);
    theme_json = sanitize(&theme_json, &names);
    let meta = metadata.get(&*deps.blocks);
    let options = NodeOptions {
        include_block_style_variations: true,
        include_node_paths_only: false,
    };
    let nodes = style_nodes(&theme_json, &meta, options, &*deps.filters)?;
    let breakpoints = viewport::media_queries(
        array_path::get(&theme_json, &["settings", "viewport"]),
        false,
    );
    let mut sanitized = Value::Array(Vec::new());
    for metadata in &nodes {
        let path = path_of(metadata);
        let input = get_path(&theme_json, &path)
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        if empty(&input) {
            continue;
        }
        let block_name = if path.iter().any(|p| p == "blocks") {
            path.get(2).cloned()
        } else {
            None
        };
        let mut output = if array_path::isset(&input, &["css"]) && security.can_edit_css() {
            input.clone()
        } else {
            security.remove_insecure_styles(&input)
        };
        if let Some(current) = path.last()
            && let Some(pseudos) = pseudo_list("VALID_ELEMENT_PSEUDO_SELECTORS", current)
        {
            for pseudo in pseudos {
                if let Some(pseudo_input) =
                    array_path::get(&input, &[&pseudo]).filter(|v| !matches!(v, Value::Null))
                {
                    put(
                        &mut output,
                        &pseudo,
                        security.remove_insecure_styles(pseudo_input),
                    );
                }
            }
        }
        for (breakpoint, _) in &breakpoints {
            let Some(bp_input) =
                array_path::get(&input, &[breakpoint]).filter(|v| !matches!(v, Value::Null))
            else {
                continue;
            };
            security.breakpoint(
                &mut output,
                breakpoint,
                bp_input,
                block_name.as_deref(),
                &breakpoints,
            );
        }
        if !empty(&output) {
            set_path(&mut sanitized, &path, output);
        }
        for (_, variation) in
            entries(array_path::get(metadata, &["variations"]).unwrap_or(&Value::Null))
        {
            let variation_path = path_of(variation);
            let variation_input = get_path(&theme_json, &variation_path)
                .cloned()
                .unwrap_or(Value::Array(Vec::new()));
            if empty(&variation_input) {
                continue;
            }
            let mut variation_output = security.remove_insecure_styles(&variation_input);
            if let Some(blocks) =
                array_path::get(&variation_input, &["blocks"]).filter(|v| !matches!(v, Value::Null))
            {
                put(
                    &mut variation_output,
                    "blocks",
                    security.remove_insecure_inner_block_styles(blocks, Some(&breakpoints)),
                );
            }
            if let Some(elements) = array_path::get(&variation_input, &["elements"])
                .filter(|v| !matches!(v, Value::Null))
            {
                put(
                    &mut variation_output,
                    "elements",
                    security.remove_insecure_element_styles(elements, Some(&breakpoints)),
                );
            }
            for (breakpoint, _) in &breakpoints {
                let Some(bp_input) = array_path::get(&variation_input, &[breakpoint])
                    .filter(|v| !matches!(v, Value::Null))
                else {
                    continue;
                };
                security.breakpoint(
                    &mut variation_output,
                    breakpoint,
                    bp_input,
                    block_name.as_deref(),
                    &breakpoints,
                );
            }
            if !empty(&variation_output) {
                set_path(&mut sanitized, &variation_path, variation_output);
            }
        }
    }
    for metadata in setting_nodes(&theme_json, &Value::Array(Vec::new())) {
        let path = path_of(&metadata);
        let input = get_path(&theme_json, &path)
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        if empty(&input) {
            continue;
        }
        let output =
            security.remove_insecure_settings(&input, path.len() == 1 && path[0] == "settings");
        if !empty(&output) {
            set_path(&mut sanitized, &path, output);
        }
    }
    for section in ["styles", "settings"] {
        match array_path::get(&sanitized, &[section])
            .filter(|v| !empty(v))
            .cloned()
        {
            None => array_path::unset(&mut theme_json, &[section]),
            Some(value) => array_path::insert(&mut theme_json, Key::Str(section.into()), value),
        }
    }
    Ok(theme_json)
}

impl Security<'_> {
    /// The breakpoint handling shared by style nodes and variations.
    fn breakpoint(
        &self,
        output: &mut Value,
        breakpoint: &str,
        input: &Value,
        block_name: Option<&str>,
        breakpoints: &[(String, String)],
    ) {
        let mut bp_output = self.remove_insecure_styles(input);
        if let Some(elements) =
            array_path::get(input, &["elements"]).filter(|v| !matches!(v, Value::Null))
        {
            put(
                &mut bp_output,
                "elements",
                self.remove_insecure_element_styles(elements, Some(breakpoints)),
            );
        }
        if let Some(blocks) =
            array_path::get(input, &["blocks"]).filter(|v| !matches!(v, Value::Null))
        {
            put(
                &mut bp_output,
                "blocks",
                self.remove_insecure_inner_block_styles(blocks, Some(breakpoints)),
            );
        }
        if let Some(name) = block_name.filter(|n| !n.is_empty() && *n != "0")
            && let Some(pseudos) = pseudo_list("VALID_BLOCK_PSEUDO_SELECTORS", name)
        {
            for pseudo in pseudos {
                if let Some(pseudo_input) =
                    array_path::get(input, &[&pseudo]).filter(|v| !matches!(v, Value::Null))
                {
                    put(
                        &mut bp_output,
                        &pseudo,
                        self.remove_insecure_styles(pseudo_input),
                    );
                }
            }
        }
        if let Some(css) = array_path::get(input, &["css"]).filter(|v| !matches!(v, Value::Null))
            && self.can_edit_css()
        {
            put(&mut bp_output, "css", css.clone());
        }
        put(output, breakpoint, bp_output);
    }
}
