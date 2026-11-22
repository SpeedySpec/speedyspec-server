//! The global styles functions (`global-styles-and-settings.php`) over the resolver, keeping WordPress's object
//! cache calls (group `theme_json`) exactly as WordPress makes them.

use std::rc::Rc;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::ThemeJsonError;
use speedyspec_wp_theme_json_contracts::ports::WpCache;

use crate::array_path;
use crate::php::to_php_string;
use crate::php_value::{empty, entries};
use crate::resolver::ThemeJsonResolver;

const GROUP: &str = "theme_json";

/// The global styles functions (Facade).
pub struct GlobalStylesApi {
    resolver: Rc<ThemeJsonResolver>,
    cache: Rc<dyn WpCache>,
}

fn steps(path: &Value) -> Vec<String> {
    entries(path)
        .iter()
        .map(|(_, v)| String::from_utf8_lossy(&to_php_string(v)).into_owned())
        .collect()
}

/// `_wp_array_get( $array, $path, $default )` with the array itself as the default.
fn at_path_or_all(array: Value, path: &[String]) -> Value {
    if path.is_empty() {
        return array;
    }
    let refs: Vec<&str> = path.iter().map(String::as_str).collect();
    match array_path::get(&array, &refs) {
        Some(v) => v.clone(),
        None => array,
    }
}

fn with_block(path: &Value, context: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(block) = array_path::get(context, &["block_name"]).filter(|v| !empty(v)) {
        out.push("blocks".to_owned());
        out.push(String::from_utf8_lossy(&to_php_string(block)).into_owned());
    }
    out.extend(steps(path));
    out
}

fn origin_of(context: &Value) -> &'static str {
    match array_path::get(context, &["origin"]) {
        Some(Value::Str(s)) if s == "base" => "theme",
        _ => "custom",
    }
}

impl GlobalStylesApi {
    /// The functions over this resolver and object cache.
    pub fn new(resolver: Rc<ThemeJsonResolver>, cache: Rc<dyn WpCache>) -> GlobalStylesApi {
        GlobalStylesApi { resolver, cache }
    }

    /// The resolver.
    pub fn resolver(&self) -> &Rc<ThemeJsonResolver> {
        &self.resolver
    }

    fn development_mode(&self) -> bool {
        self.resolver.deps().theme.is_development_mode()
    }

    /// `wp_get_global_settings( $path, $context )`.
    pub fn get_global_settings(
        &self,
        path: &Value,
        context: &Value,
    ) -> Result<Value, ThemeJsonError> {
        let path = with_block(path, context);
        let origin = origin_of(context);
        let key = format!("wp_get_global_settings_{origin}");
        let can_use_cached = !self.development_mode();
        let mut settings = None;
        if can_use_cached {
            settings = self
                .cache
                .get(&key, GROUP)
                .filter(|v| !matches!(v, Value::Bool(false)));
        }
        let settings = match settings {
            Some(s) => s,
            None => {
                let merged = self.resolver.get_merged_data(&Value::Str(origin.into()))?;
                let s = self.resolver.api().get_settings(&merged);
                if can_use_cached {
                    self.cache.set(&key, s.clone(), GROUP);
                }
                s
            }
        };
        Ok(at_path_or_all(settings, &path))
    }

    /// `wp_get_global_styles( $path, $context )`.
    pub fn get_global_styles(
        &self,
        path: &Value,
        context: &Value,
    ) -> Result<Value, ThemeJsonError> {
        let path = with_block(path, context);
        let origin = origin_of(context);
        let resolve = matches!(array_path::get(context, &["transforms"]), Some(Value::Array(t))
            if t.iter().any(|(_, v)| matches!(v, Value::Str(s) if s == "resolve-variables")));
        let mut merged = self.resolver.get_merged_data(&Value::Str(origin.into()))?;
        if resolve {
            merged = self.resolver.api().resolve_variables(&merged);
        }
        let raw = self.resolver.api().get_raw_data(&merged);
        let styles = array_path::get(&raw, &["styles"])
            .cloned()
            .unwrap_or(Value::Null);
        Ok(at_path_or_all(styles, &path))
    }

    /// `wp_get_global_stylesheet( $types )`.
    pub fn get_global_stylesheet(&self, types: &Value) -> Result<Vec<u8>, ThemeJsonError> {
        let can_use_cached = empty(types) && !self.development_mode();
        let key = "wp_get_global_stylesheet";
        if can_use_cached && let Some(cached) = self.cache.get(key, GROUP).filter(|v| !empty(v)) {
            return Ok(to_php_string(&cached));
        }
        let api = self.resolver.api();
        let tree = self.resolver.resolve_theme_file_uris(
            &self
                .resolver
                .get_merged_data(&Value::Str("custom".into()))?,
        );
        let mut types: Vec<Value> = if empty(types) {
            ["variables", "styles", "presets"]
                .iter()
                .map(|t| Value::Str((*t).into()))
                .collect()
        } else {
            entries(types).iter().map(|(_, v)| v.clone()).collect()
        };
        let theme = &*self.resolver.deps().theme;
        let mut options = Vec::new();
        if !theme.is_block_theme() && !theme.has_theme_json() {
            options.push((Key::Str("base_layout_styles".into()), Value::Bool(true)));
        }
        let options = Value::Array(options);
        let origins = Value::Array(
            ["default", "theme", "custom"]
                .iter()
                .enumerate()
                .map(|(i, o)| (Key::Int(i as i64), Value::Str((*o).into())))
                .collect(),
        );
        let list = |items: &[Value]| {
            Value::Array(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (Key::Int(i as i64), v.clone()))
                    .collect(),
            )
        };
        let is_variables = |v: &Value| matches!(v, Value::Str(s) if s == "variables");
        let mut stylesheet = Vec::new();
        if types.iter().any(is_variables) {
            stylesheet.extend(api.get_stylesheet(
                &tree,
                &list(&[Value::Str("variables".into())]),
                &origins,
                &options,
            )?);
            types.retain(|t| to_php_string(t) != b"variables");
        }
        if !types.is_empty() {
            stylesheet.extend(api.get_stylesheet(&tree, &list(&types), &origins, &options)?);
        }
        if can_use_cached {
            self.cache
                .set(key, crate::codec::string_value(&stylesheet), GROUP);
        }
        Ok(stylesheet)
    }

    /// `wp_get_block_name_from_theme_json_path( $path )`.
    pub fn get_block_name_from_theme_json_path(path: &Value) -> Value {
        let parts = steps(path);
        if parts.len() >= 3
            && parts[0] == "styles"
            && parts[1] == "blocks"
            && parts[2].contains('/')
        {
            return Value::Str(parts[2].clone());
        }
        parts
            .into_iter()
            .find(|p| p.contains("core/"))
            .map_or(Value::Str(String::new()), Value::Str)
    }

    fn theme_data_without_supports(
        &self,
    ) -> Result<speedyspec_wp_theme_json_contracts::model::ThemeJson, ThemeJsonError> {
        self.resolver
            .get_theme_data(&Value::Array(Vec::new()), false)
    }

    /// `wp_get_theme_directory_pattern_slugs()`.
    pub fn get_theme_directory_pattern_slugs(&self) -> Result<Value, ThemeJsonError> {
        Ok(self
            .resolver
            .api()
            .get_patterns(&self.theme_data_without_supports()?))
    }

    /// `wp_get_theme_data_custom_templates()`.
    pub fn get_theme_data_custom_templates(&self) -> Result<Value, ThemeJsonError> {
        Ok(self
            .resolver
            .api()
            .get_custom_templates(&self.theme_data_without_supports()?))
    }

    /// `wp_get_theme_data_template_parts()`.
    pub fn get_theme_data_template_parts(&self) -> Result<Value, ThemeJsonError> {
        let key = "wp_get_theme_data_template_parts";
        let can_use_cached = !self.development_mode();
        if can_use_cached
            && let Some(cached) = self
                .cache
                .get(key, GROUP)
                .filter(|v| !matches!(v, Value::Bool(false)))
        {
            return Ok(cached);
        }
        let parts = self
            .resolver
            .api()
            .get_template_parts(&self.theme_data_without_supports()?);
        if can_use_cached {
            self.cache.set(key, parts.clone(), GROUP);
        }
        Ok(parts)
    }

    /// `wp_clean_theme_json_cache()`.
    pub fn clean_theme_json_cache(&self) {
        for key in [
            "wp_get_global_stylesheet",
            "wp_get_global_styles_svg_filters",
            "wp_get_global_settings_custom",
            "wp_get_global_settings_theme",
            "wp_get_global_styles_custom_css",
            "wp_get_theme_data_template_parts",
        ] {
            self.cache.delete(key, GROUP);
        }
        self.resolver.clean_cached_data();
    }
}

/// `wp_get_global_settings()` as the engine's `GlobalSettings` port: bound after the global styles API exists (the
/// API needs the engine, and the engine's fluid typography needs the API), as WordPress's function is global.
#[derive(Default)]
pub struct ResolverGlobalSettings {
    api: std::cell::RefCell<std::rc::Weak<GlobalStylesApi>>,
}

impl ResolverGlobalSettings {
    /// Answers from `api` from now on.
    pub fn bind(&self, api: &Rc<GlobalStylesApi>) {
        *self.api.borrow_mut() = Rc::downgrade(api);
    }
}

impl speedyspec_wp_theme_json_contracts::ports::GlobalSettings for ResolverGlobalSettings {
    fn settings(&self) -> Value {
        match self.api.borrow().upgrade() {
            Some(api) => api
                .get_global_settings(&Value::Array(Vec::new()), &Value::Array(Vec::new()))
                .unwrap_or(Value::Array(Vec::new())),
            None => Value::Array(Vec::new()),
        }
    }
}
