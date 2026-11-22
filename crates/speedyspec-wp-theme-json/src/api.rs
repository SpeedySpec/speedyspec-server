//! `ThemeJsonApi`: the one entry point behind `WP_Theme_JSON`, one method per public method (Facade).

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::model::ThemeJson;
use speedyspec_wp_theme_json_contracts::Origin;

use crate::array_path::{self, child};
use crate::php::{Num, to_number};
use crate::presets::spacing::compute_spacing_sizes;
use crate::{ThemeJsonCodec, ThemeJsonDeps, data, schema_migrate};

/// `WP_Theme_JSON`'s public methods over the typed model.
pub struct ThemeJsonApi {
    deps: ThemeJsonDeps,
}

impl ThemeJsonApi {
    /// The API over these ports.
    pub fn new(deps: ThemeJsonDeps) -> ThemeJsonApi {
        ThemeJsonApi { deps }
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
        ThemeJsonCodec::decode(&crate::merge::merge_value(ThemeJsonCodec::encode(base), incoming, &self.deps))
    }

    /// `WP_Theme_JSON::get_viewport_media_queries( $viewport_settings, $options )` as `(state, query)` pairs.
    pub fn get_viewport_media_queries(&self, viewport: &Value, include_desktop: bool) -> Vec<(String, String)> {
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
        self.deps.diagnostics.deprecated_function(METHOD, "6.6.0", "");
        let mut raw = ThemeJsonCodec::encode(doc);
        let scale = array_path::get(&raw, &["settings", "spacing", "spacingScale"])
            .filter(|s| !matches!(s, Value::Null))
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let field = |name: &str| child(&scale, &Key::Str(name.into())).filter(|v| !matches!(v, Value::Null));
        let numeric = |name: &str| field(name).and_then(to_number);
        let operator_ok = matches!(field("operator"), Some(Value::Str(o)) if o == "+" || o == "*");
        let valid = numeric("steps").is_some()
            && numeric("mediumStep").is_some()
            && field("unit").is_some()
            && numeric("increment").is_some()
            && operator_ok;
        if !valid {
            if crate::merge::truthy(&scale) {
                let template = self.deps.gettext.translate("Some of the %1$s %2$s values are invalid", "default");
                let message = String::from_utf8_lossy(&template)
                    .replacen("%1$s", "theme.json", 1)
                    .replacen("%2$s", "settings.spacing.spacingScale", 1);
                self.deps.diagnostics.trigger_error(METHOD, message.as_bytes());
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
                array_path::insert(item, Key::Str("name".into()), Value::Str((i + 1).to_string()));
            }
        }
        array_path::set(&mut raw, &["settings", "spacing", "spacingSizes", "default"], sizes);
        *doc = ThemeJsonCodec::decode(&raw);
    }
}
