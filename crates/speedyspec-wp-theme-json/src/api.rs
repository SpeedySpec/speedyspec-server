//! `ThemeJsonApi`: the one entry point behind `WP_Theme_JSON`, one method per public method (Facade).

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::ports::RegisteredBlock;
use speedyspec_wp_theme_json_contracts::{Origin, ThemeJsonError};

use std::rc::Rc;

use crate::nodes::{NodeOptions, block_nodes};
use crate::presets::FluidFontSize;
use crate::selectors::BlocksMetadataCache;
use crate::sheet::Sheet;
use crate::stylesheet::{stylesheet_origins, stylesheet_types};
use speedyspec_wp_theme_json_contracts::model::ThemeJson;

use crate::array_path::{self, child};
use crate::php::{Num, to_number};
use crate::presets::spacing::compute_spacing_sizes;
use crate::{ThemeJsonCodec, ThemeJsonDeps, data, schema_migrate};

/// `WP_Theme_JSON`'s public methods over the typed model.
pub struct ThemeJsonApi {
    deps: ThemeJsonDeps,
    blocks_metadata: BlocksMetadataCache,
}

impl ThemeJsonApi {
    /// The API over these ports.
    pub fn new(deps: ThemeJsonDeps) -> ThemeJsonApi {
        ThemeJsonApi {
            deps,
            blocks_metadata: BlocksMetadataCache::default(),
        }
    }

    /// The ports.
    pub fn deps(&self) -> &ThemeJsonDeps {
        &self.deps
    }

    /// `new WP_Theme_JSON( $theme_json, $origin )`.
    pub fn construct(&self, doc: Value, origin: &[u8]) -> ThemeJson {
        crate::construct(doc, origin, &self.deps)
    }

    /// `$this->merge( $incoming )`.
    pub fn merge(&self, base: &ThemeJson, incoming: &ThemeJson) -> ThemeJson {
        crate::merge(base, incoming, &self.deps)
    }

    /// `$this->merge( $incoming )` where only `$incoming->get_raw_data()` is known, as WordPress reads it.
    pub fn merge_raw_data(&self, base: &ThemeJson, incoming: Value) -> ThemeJson {
        ThemeJsonCodec::decode(&crate::merge::merge_value(
            ThemeJsonCodec::encode(base),
            incoming,
            &self.deps,
        ))
    }

    /// `WP_Theme_JSON::get_viewport_media_queries( $viewport_settings, $options )` as `(state, query)` pairs.
    pub fn get_viewport_media_queries(
        &self,
        viewport: &Value,
        include_desktop: bool,
    ) -> Vec<(String, String)> {
        let viewport = (!matches!(viewport, Value::Null)).then_some(viewport);
        crate::sanitize::viewport::media_queries(viewport, include_desktop)
    }

    /// `get_raw_data()`.
    pub fn get_raw_data(&self, doc: &ThemeJson) -> Value {
        ThemeJsonCodec::encode(doc)
    }

    /// `get_settings()`.
    pub fn get_settings(&self, doc: &ThemeJson) -> Value {
        data::settings(&ThemeJsonCodec::encode(doc))
    }

    /// `get_data()`.
    pub fn get_data(&self, doc: &ThemeJson) -> Value {
        data::get_data(&ThemeJsonCodec::encode(doc))
    }

    /// `get_custom_templates()`.
    pub fn get_custom_templates(&self, doc: &ThemeJson) -> Value {
        data::custom_templates(&ThemeJsonCodec::encode(doc))
    }

    /// `get_template_parts()`.
    pub fn get_template_parts(&self, doc: &ThemeJson) -> Value {
        data::template_parts(&ThemeJsonCodec::encode(doc))
    }

    /// `get_patterns()`.
    pub fn get_patterns(&self, doc: &ThemeJson) -> Value {
        data::patterns(&ThemeJsonCodec::encode(doc))
    }

    /// `WP_Theme_JSON::get_from_editor_settings( $settings )`.
    pub fn get_from_editor_settings(&self, settings: &Value) -> Value {
        data::from_editor_settings(settings)
    }

    /// `WP_Theme_JSON::get_element_class_name( $element )`.
    pub fn get_element_class_name(&self, element: &[u8]) -> Vec<u8> {
        data::element_class_name(element)
    }

    /// `WP_Theme_JSON_Schema::migrate( $theme_json, $origin )`.
    pub fn schema_migrate(&self, doc: Value, origin: &[u8]) -> Value {
        schema_migrate::migrate(doc, Origin::parse(origin).unwrap_or(Origin::Theme))
    }

    /// `set_spacing_sizes()` (deprecated in WordPress 6.6): writes default spacing sizes from an unkeyed scale.
    ///
    /// After construction the scale is keyed by origin, so WordPress reports it invalid; this keeps that behaviour.
    pub fn set_spacing_sizes(&self, doc: &mut ThemeJson) {
        const METHOD: &str = "WP_Theme_JSON::set_spacing_sizes";
        self.deps
            .diagnostics
            .deprecated_function(METHOD, "6.6.0", "");
        let mut raw = ThemeJsonCodec::encode(doc);
        let scale = array_path::get(&raw, &["settings", "spacing", "spacingScale"])
            .filter(|s| !matches!(s, Value::Null))
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let field = |name: &str| {
            child(&scale, &Key::Str(name.into())).filter(|v| !matches!(v, Value::Null))
        };
        let numeric = |name: &str| field(name).and_then(to_number);
        let operator_ok = matches!(field("operator"), Some(Value::Str(o)) if o == "+" || o == "*");
        let valid = numeric("steps").is_some()
            && numeric("mediumStep").is_some()
            && field("unit").is_some()
            && numeric("increment").is_some()
            && operator_ok;
        if !valid {
            if crate::merge::truthy(&scale) {
                let template = self
                    .deps
                    .gettext
                    .translate("Some of the %1$s %2$s values are invalid", "default");
                let message = String::from_utf8_lossy(&template)
                    .replacen("%1$s", "theme.json", 1)
                    .replacen("%2$s", "settings.spacing.spacingScale", 1);
                self.deps
                    .diagnostics
                    .trigger_error(METHOD, message.as_bytes());
            }
            return;
        }
        if matches!(field("steps"), Some(Value::Int(0))) {
            return;
        }
        let mut sizes = compute_spacing_sizes(&scale, &self.deps);
        if numeric("steps").is_some_and(|s| s.as_f64() <= Num::Int(7).as_f64())
            && let Value::Array(items) = &mut sizes
        {
            for (i, (_, item)) in items.iter_mut().enumerate() {
                array_path::insert(
                    item,
                    Key::Str("name".into()),
                    Value::Str((i + 1).to_string()),
                );
            }
        }
        array_path::set(
            &mut raw,
            &["settings", "spacing", "spacingSizes", "default"],
            sizes,
        );
        *doc = ThemeJsonCodec::decode(&raw);
    }

    /// The registered blocks' computed metadata (`get_blocks_metadata()`), recomputed when the registries change.
    pub fn blocks_metadata(&self) -> Rc<Value> {
        self.blocks_metadata.get(&*self.deps.blocks)
    }

    fn with_sheet<T>(&self, doc: &ThemeJson, f: impl FnOnce(&Sheet<'_>) -> T) -> T {
        let raw = ThemeJsonCodec::encode(doc);
        let sheet = Sheet::new(&raw, &self.deps, self.blocks_metadata());
        f(&sheet)
    }

    /// `get_stylesheet( $types, $origins, $options )`.
    pub fn get_stylesheet(
        &self,
        doc: &ThemeJson,
        types: &Value,
        origins: &Value,
        options: &Value,
    ) -> Result<Vec<u8>, ThemeJsonError> {
        self.with_sheet(doc, |sheet| {
            let types = stylesheet_types(types, sheet);
            sheet.stylesheet(&types, &stylesheet_origins(origins), options)
        })
    }

    /// `get_styles_for_block( $block_metadata )`.
    pub fn get_styles_for_block(&self, doc: &ThemeJson, metadata: &Value) -> Vec<u8> {
        self.with_sheet(doc, |sheet| sheet.styles_for_block(metadata))
    }

    /// `get_styles_block_nodes()`.
    pub fn get_styles_block_nodes(&self, doc: &ThemeJson) -> Value {
        let raw = ThemeJsonCodec::encode(doc);
        let nodes = block_nodes(&raw, &self.blocks_metadata(), NodeOptions::default());
        Value::Array(
            nodes
                .into_iter()
                .enumerate()
                .map(|(i, n)| (Key::Int(i as i64), n))
                .collect(),
        )
    }

    /// `get_root_layout_rules( $selector, $block_metadata, $options )`.
    pub fn get_root_layout_rules(
        &self,
        doc: &ThemeJson,
        metadata: &Value,
        options: &Value,
    ) -> Vec<u8> {
        let base_layout_styles =
            array_path::get(options, &["base_layout_styles"]).is_some_and(crate::merge::truthy);
        self.with_sheet(doc, |sheet| {
            sheet.root_layout_rules(metadata, base_layout_styles)
        })
    }

    /// `get_custom_css()` (deprecated in 6.7).
    pub fn get_custom_css(&self, doc: &ThemeJson) -> Vec<u8> {
        self.with_sheet(doc, |sheet| sheet.custom_css())
    }

    /// `WP_Theme_JSON::scope_selector( $scope, $selector )`: `None` for WordPress's `null` selector.
    pub fn scope_selector(&self, scope: &str, selector: Option<&str>) -> Option<String> {
        selector.map(|s| crate::selectors::scope_selector(scope, s))
    }

    /// `WP_Theme_JSON::process_blocks_custom_css( $css, $selector )`.
    pub fn process_blocks_custom_css(&self, css: &str, selector: &str) -> String {
        crate::stylesheet::process_blocks_custom_css(css, selector)
    }

    /// `wp_get_block_css_selector( $block_type, $target, $fallback )`.
    pub fn block_css_selector(
        &self,
        block: &RegisteredBlock,
        target: &Value,
        fallback: bool,
    ) -> Option<String> {
        crate::selectors::block_css_selector(block, target, fallback)
    }

    /// `WP_Theme_JSON::remove_insecure_properties( $theme_json, $origin )`.
    pub fn remove_insecure_properties(
        &self,
        doc: Value,
        origin: &Value,
    ) -> Result<Value, ThemeJsonError> {
        crate::security::remove_insecure_properties(doc, origin, &self.deps, &self.blocks_metadata)
    }

    /// `get_svg_filters( $origins )`.
    pub fn get_svg_filters(&self, doc: &ThemeJson, origins: &Value) -> Vec<u8> {
        let raw = ThemeJsonCodec::encode(doc);
        // Unlike get_stylesheet, `null` here is no origins (WordPress loops over it).
        let origins: Vec<Origin> = crate::php_value::entries(origins)
            .iter()
            .filter_map(|(_, o)| {
                crate::php_value::as_str(o).and_then(|s| Origin::parse(s.as_bytes()))
            })
            .collect();
        crate::duotone::svg_filters(&raw, &self.blocks_metadata(), &origins, &self.deps)
    }

    /// `WP_Theme_JSON::resolve_variables( $theme_json )`: the document with its styles' preset and custom variables
    /// replaced by their values.
    pub fn resolve_variables(&self, doc: &ThemeJson) -> ThemeJson {
        let mut raw = ThemeJsonCodec::encode(doc);
        let settings = data::settings(&raw);
        let fluid = FluidFontSize {
            global: self.deps.global_settings.clone(),
            diagnostics: self.deps.diagnostics.clone(),
        };
        let values = crate::declarations::variable_values(&settings, &fluid);
        let styles = array_path::get(&raw, &["styles"])
            .cloned()
            .unwrap_or(Value::Null);
        array_path::insert(
            &mut raw,
            Key::Str("styles".into()),
            crate::declarations::convert_variables_to_value(styles, &values),
        );
        ThemeJsonCodec::decode(&raw)
    }

    /// `get_metadata_boolean( $data, $path, $default_value )` (protected in WordPress).
    pub fn get_metadata_boolean(&self, data: &Value, path: &Value, default_value: Value) -> Value {
        match path {
            Value::Bool(_) => path.clone(),
            Value::Array(steps) => {
                let steps: Vec<String> = steps
                    .iter()
                    .map(|(_, v)| {
                        String::from_utf8_lossy(&crate::php::to_php_string(v)).into_owned()
                    })
                    .collect();
                let refs: Vec<&str> = steps.iter().map(String::as_str).collect();
                match array_path::get(data, &refs) {
                    Some(v) if !matches!(v, Value::Null) => v.clone(),
                    _ => default_value,
                }
            }
            _ => default_value,
        }
    }

    /// `should_override_preset( $theme_json, $path, $override )` (protected and deprecated in WordPress).
    pub fn should_override_preset(
        &self,
        theme_json: &Value,
        path: &[String],
        override_: &Value,
    ) -> Value {
        self.deps.diagnostics.deprecated_function(
            "WP_Theme_JSON::should_override_preset",
            "6.0.0",
            "get_metadata_boolean",
        );
        match override_ {
            Value::Bool(_) => override_.clone(),
            Value::Array(steps) => {
                let steps: Vec<String> = steps
                    .iter()
                    .map(|(_, v)| {
                        String::from_utf8_lossy(&crate::php::to_php_string(v)).into_owned()
                    })
                    .collect();
                let lookup = |prefix: &[String]| {
                    let full: Vec<&str> = prefix
                        .iter()
                        .chain(steps.iter())
                        .map(String::as_str)
                        .collect();
                    array_path::get(theme_json, &full)
                        .filter(|v| !matches!(v, Value::Null))
                        .cloned()
                };
                if let Some(value) = lookup(path) {
                    return Value::Bool(!crate::merge::truthy(&value));
                }
                if let Some(value) = lookup(&["settings".to_owned()]) {
                    return Value::Bool(!crate::merge::truthy(&value));
                }
                Value::Bool(true)
            }
            _ => Value::Null,
        }
    }
}
