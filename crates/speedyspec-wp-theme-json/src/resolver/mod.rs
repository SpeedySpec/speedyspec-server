//! `WP_Theme_JSON_Resolver` (Facade over Repositories per origin): core, blocks, theme and user data, each read,
//! translated where WordPress translates it, passed through its `wp_theme_json_data_*` filter and kept for the
//! request as WordPress's statics keep it, then merged in WordPress's order.

pub mod files;
mod translate;
mod variations;

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::model::ThemeJson;
use speedyspec_wp_theme_json_contracts::ports::{
    ActiveTheme, BlockStyleVariationRegistrar, FilteredLayer, GlobalStylesPost, LayerFilters,
};
use speedyspec_wp_theme_json_contracts::{Origin, ThemeJsonError};

pub use translate::translate;

use crate::array_path;
use crate::php_value::{empty, entries, key_str};
use crate::{ThemeJsonApi, ThemeJsonCodec};
use files::FileCache;

/// What the resolver reads from outside, beyond the engine's own ports.
#[derive(Clone)]
pub struct ResolverDeps {
    /// The active theme.
    pub theme: Rc<dyn ActiveTheme>,
    /// The data filters.
    pub filters: Rc<dyn LayerFilters>,
    /// The user's global styles post.
    pub posts: Rc<dyn GlobalStylesPost>,
    /// Registers block style variations from the theme's partials.
    pub registrar: Rc<dyn BlockStyleVariationRegistrar>,
    /// The directory holding core's `theme.json` (`wp-includes`); empty to ask the theme port each time.
    pub core_dir: String,
}

/// The origins WordPress's `$blocks_cache` tracks; `merged` is the merged data's (WordPress 7.2).
const CACHE_ORIGINS: [&str; 5] = ["core", "blocks", "theme", "user", "merged"];

/// WordPress's per-request statics.
#[derive(Default)]
struct State {
    core: Option<ThemeJson>,
    blocks: Option<ThemeJson>,
    theme: Option<ThemeJson>,
    user: Option<ThemeJson>,
    /// `$merged`: the merged data by origin, for the request.
    merged: Vec<(&'static str, ThemeJson)>,
    blocks_cache: Vec<(&'static str, Vec<String>)>,
    user_post_id: Option<Value>,
}

impl State {
    fn fresh() -> State {
        State {
            blocks_cache: CACHE_ORIGINS.iter().map(|o| (*o, Vec::new())).collect(),
            ..State::default()
        }
    }
}

/// `WP_Theme_JSON_Resolver`.
pub struct ThemeJsonResolver {
    api: Rc<ThemeJsonApi>,
    deps: ResolverDeps,
    state: RefCell<State>,
    files: FileCache,
}

impl ThemeJsonResolver {
    /// A resolver over the engine and these sources.
    pub fn new(api: Rc<ThemeJsonApi>, deps: ResolverDeps) -> ThemeJsonResolver {
        ThemeJsonResolver {
            api,
            deps,
            state: RefCell::new(State::fresh()),
            files: FileCache::default(),
        }
    }

    /// The engine.
    pub fn api(&self) -> &Rc<ThemeJsonApi> {
        &self.api
    }

    /// The resolver's ports.
    pub fn deps(&self) -> &ResolverDeps {
        &self.deps
    }

    fn read_json_file(&self, file: &str) -> Value {
        let d = self.api.deps();
        self.files.read(file, &*d.diagnostics, &*d.gettext)
    }

    fn translate(&self, data: Value, domain: &Value) -> Value {
        translate(data, domain, &*self.api.deps().gettext)
    }

    /// `has_same_registered_blocks( $origin )`.
    fn has_same_registered_blocks(&self, origin: &str) -> bool {
        let names: Vec<String> = self
            .api
            .deps()
            .blocks
            .registered_now()
            .into_iter()
            .map(|b| b.name)
            .collect();
        let mut state = self.state.borrow_mut();
        let Some((_, seen)) = state.blocks_cache.iter_mut().find(|(o, _)| *o == origin) else {
            return false;
        };
        if names.iter().all(|n| seen.contains(n)) {
            return true;
        }
        for name in names {
            if !seen.contains(&name) {
                seen.push(name);
            }
        }
        false
    }

    fn filtered(
        &self,
        hook: &str,
        origin: Origin,
        config: Value,
    ) -> Result<ThemeJson, ThemeJsonError> {
        Ok(match self.deps.filters.apply(hook, origin, config)? {
            FilteredLayer::Built(raw) => ThemeJsonCodec::decode(&raw),
            FilteredLayer::Config(config) => self.api.construct(config, origin.as_str().as_bytes()),
        })
    }

    /// `get_core_data()`.
    pub fn get_core_data(&self) -> Result<ThemeJson, ThemeJsonError> {
        if self.state.borrow().core.is_some() && self.has_same_registered_blocks("core") {
            return Ok(self.state.borrow().core.clone().unwrap_or_default());
        }
        let core_dir = if self.deps.core_dir.is_empty() {
            self.deps.theme.core_directory()
        } else {
            self.deps.core_dir.clone()
        };
        let config = self.read_json_file(&format!("{core_dir}/theme.json"));
        let config = self.translate(config, &Value::Str("default".into()));
        let core = self.filtered("wp_theme_json_data_default", Origin::Default, config)?;
        self.state.borrow_mut().core = Some(core.clone());
        Ok(core)
    }

    /// `get_block_data()`.
    pub fn get_block_data(&self) -> Result<ThemeJson, ThemeJsonError> {
        let blocks = self.api.deps().blocks.registered_now();
        if self.state.borrow().blocks.is_some() && self.has_same_registered_blocks("blocks") {
            return Ok(self.state.borrow().blocks.clone().unwrap_or_default());
        }
        let mut config = Value::Array(vec![(Key::Str("version".into()), Value::Int(3))]);
        for block in &blocks {
            let name = block.name.as_str();
            if let Some(style) = array_path::get(&block.supports, &["__experimentalStyle"])
                .filter(|v| !matches!(v, Value::Null))
            {
                array_path::set(
                    &mut config,
                    &["styles", "blocks", name],
                    remove_json_comments(style.clone()),
                );
            }
            if array_path::isset(
                &block.supports,
                &["spacing", "blockGap", "__experimentalDefault"],
            ) && !array_path::isset(&config, &["styles", "blocks", name, "spacing", "blockGap"])
            {
                array_path::set(
                    &mut config,
                    &["styles", "blocks", name, "spacing", "blockGap"],
                    Value::Null,
                );
            }
        }
        let built = self.filtered("wp_theme_json_data_blocks", Origin::Blocks, config)?;
        self.state.borrow_mut().blocks = Some(built.clone());
        Ok(built)
    }

    /// `get_theme_data( $deprecated, $options )`: with `with_supports` (the default) the theme supports of classic
    /// themes are merged under the theme's data.
    pub fn get_theme_data(
        &self,
        deprecated: &Value,
        with_supports: bool,
    ) -> Result<ThemeJson, ThemeJsonError> {
        let d = self.api.deps();
        if !empty(deprecated) {
            d.diagnostics.deprecated_argument(
                "WP_Theme_JSON_Resolver::get_theme_data",
                "5.9.0",
                b"",
            );
        }
        if self.state.borrow().theme.is_none() || !self.has_same_registered_blocks("theme") {
            let theme = &*self.deps.theme;
            let file = theme.theme_json_path();
            let mut data = if files::is_readable(&file) {
                let data = self.read_json_file(&file);
                self.translate(data, &theme.text_domain())
            } else {
                Value::Array(vec![(Key::Str("version".into()), Value::Int(3))])
            };
            let block_variations = self.get_style_variations("block");
            self.deps.registrar.register(&block_variations);
            data = variations::inject_from_files(data, &block_variations);
            data = variations::inject_from_registry(data, &d.blocks.registered_styles());
            let mut built = self.filtered("wp_theme_json_data_theme", Origin::Theme, data)?;
            if let Some(parent_file) = theme.parent_theme_json_path()
                && parent_file != file
                && files::is_readable(&parent_file)
            {
                let parent_data = self.read_json_file(&parent_file);
                let parent_data = self.translate(parent_data, &theme.parent_text_domain());
                let parent = self.api.construct(parent_data, b"theme");
                built = self.api.merge(&parent, &built);
            }
            self.state.borrow_mut().theme = Some(built);
        }
        let theme_data = self.state.borrow().theme.clone().unwrap_or_default();
        if !with_supports {
            return Ok(theme_data);
        }
        let theme = &*self.deps.theme;
        let mut support = self
            .api
            .get_from_editor_settings(&theme.classic_editor_settings());
        if !theme.has_theme_json() {
            let unset_or = |data: &Value, path: &[&str], feature: &str| {
                Value::Bool(!array_path::isset(data, path) || theme.supports(feature))
            };
            let palette = unset_or(
                &support,
                &["settings", "color", "palette"],
                "default-color-palette",
            );
            array_path::set(
                &mut support,
                &["settings", "color", "defaultPalette"],
                palette,
            );
            let gradients = unset_or(
                &support,
                &["settings", "color", "gradients"],
                "default-gradient-presets",
            );
            array_path::set(
                &mut support,
                &["settings", "color", "defaultGradients"],
                gradients,
            );
            let sizes = unset_or(
                &support,
                &["settings", "typography", "fontSizes"],
                "default-font-sizes",
            );
            array_path::set(
                &mut support,
                &["settings", "typography", "defaultFontSizes"],
                sizes,
            );
            let spacing = unset_or(
                &support,
                &["settings", "spacing", "spacingSizes"],
                "default-spacing-sizes",
            );
            array_path::set(
                &mut support,
                &["settings", "spacing", "defaultSpacingSizes"],
                spacing,
            );
            array_path::set(
                &mut support,
                &["settings", "shadow", "defaultPresets"],
                Value::Bool(false),
            );
            if theme.supports("link-color") {
                array_path::set(
                    &mut support,
                    &["settings", "color", "link"],
                    Value::Bool(true),
                );
            }
            if theme.supports("border") {
                for side in ["color", "radius", "style", "width"] {
                    array_path::set(
                        &mut support,
                        &["settings", "border", side],
                        Value::Bool(true),
                    );
                }
            }
            if theme.supports("appearance-tools") {
                array_path::set(
                    &mut support,
                    &["settings", "appearanceTools"],
                    Value::Bool(true),
                );
            }
        }
        let with_theme_supports = self.api.construct(support, b"theme");
        Ok(self.api.merge(&with_theme_supports, &theme_data))
    }

    /// `get_user_data_from_wp_global_styles( $theme, $create_post, $post_status_filter )`.
    pub fn get_user_data_from_wp_global_styles(
        &self,
        stylesheet: Option<&str>,
        create: bool,
        statuses: &Value,
    ) -> Value {
        let stylesheet = stylesheet.map_or_else(|| self.deps.theme.stylesheet(), str::to_owned);
        self.deps.posts.find(&stylesheet, create, statuses)
    }

    fn publish() -> Value {
        Value::Array(vec![(Key::Int(0), Value::Str("publish".into()))])
    }

    /// `get_user_data()`.
    pub fn get_user_data(&self) -> Result<ThemeJson, ThemeJsonError> {
        if self.state.borrow().user.is_some() && self.has_same_registered_blocks("user") {
            return Ok(self.state.borrow().user.clone().unwrap_or_default());
        }
        let mut config = Value::Array(Vec::new());
        let post = self.get_user_data_from_wp_global_styles(None, false, &Self::publish());
        if let Value::Array(fields) = &post
            && let Some((_, content)) = fields
                .iter()
                .find(|(k, _)| *k == Key::Str("post_content".into()))
        {
            match crate::php_json::decode(&crate::php::to_php_string(content)) {
                Err(e) => {
                    let message = format!(
                        "Error when decoding a theme.json schema for user data. {}",
                        e.0
                    );
                    self.api
                        .deps()
                        .diagnostics
                        .trigger_error("WP_Theme_JSON_Resolver::get_user_data", message.as_bytes());
                    return self.filtered(
                        "wp_theme_json_data_user",
                        Origin::Custom,
                        Value::Array(Vec::new()),
                    );
                }
                Ok(mut decoded @ Value::Array(_)) => {
                    if array_path::get(&decoded, &["isGlobalStylesUserThemeJSON"])
                        .is_some_and(|v| !empty(v))
                    {
                        array_path::unset(&mut decoded, &["isGlobalStylesUserThemeJSON"]);
                        config = decoded;
                    }
                }
                Ok(_) => {}
            }
        }
        let user = self.filtered("wp_theme_json_data_user", Origin::Custom, config)?;
        self.state.borrow_mut().user = Some(user.clone());
        Ok(user)
    }

    /// `get_merged_data( $origin )`: core, then blocks, theme and user, stopping at `origin`. The result is cached per
    /// origin for the request until a block is registered or the cached data is cleaned, and each caller gets its own
    /// copy (WordPress 7.2).
    pub fn get_merged_data(&self, origin: &Value) -> Result<ThemeJson, ThemeJsonError> {
        if let Value::Array(_) = origin {
            self.api
                .deps()
                .diagnostics
                .deprecated_argument("get_merged_data", "5.9.0", b"");
        }
        let origin = match origin {
            Value::Str(s) if s == "default" => "default",
            Value::Str(s) if s == "blocks" => "blocks",
            Value::Str(s) if s == "theme" => "theme",
            _ => "custom",
        };
        if !self.has_same_registered_blocks("merged") {
            self.state.borrow_mut().merged.clear();
        }
        if let Some((_, cached)) = self
            .state
            .borrow()
            .merged
            .iter()
            .find(|(o, _)| *o == origin)
        {
            return Ok(cached.clone());
        }
        let mut result = self.api.construct(
            Value::Array(vec![(Key::Str("version".into()), Value::Int(3))]),
            b"theme",
        );
        result = self.api.merge(&result, &self.get_core_data()?);
        if origin != "default" {
            result = self.api.merge(&result, &self.get_block_data()?);
        }
        if origin == "theme" || origin == "custom" {
            result = self.api.merge(
                &result,
                &self.get_theme_data(&Value::Array(Vec::new()), true)?,
            );
        }
        if origin == "custom" {
            result = self.api.merge(&result, &self.get_user_data()?);
        }
        self.state
            .borrow_mut()
            .merged
            .push((origin, result.clone()));
        Ok(result)
    }

    /// `get_user_global_styles_post_id()`.
    pub fn get_user_global_styles_post_id(&self) -> Value {
        if let Some(id) = self.state.borrow().user_post_id.clone() {
            return id;
        }
        let post = self.get_user_data_from_wp_global_styles(None, true, &Self::publish());
        if let Value::Array(fields) = &post
            && let Some((_, id)) = fields.iter().find(|(k, _)| *k == Key::Str("ID".into()))
        {
            self.state.borrow_mut().user_post_id = Some(id.clone());
            return id.clone();
        }
        Value::Null
    }

    /// `theme_has_support()` (deprecated in 6.2).
    pub fn theme_has_support(&self) -> bool {
        self.api.deps().diagnostics.deprecated_function(
            "WP_Theme_JSON_Resolver::theme_has_support",
            "6.2.0",
            "wp_theme_has_theme_json()",
        );
        self.deps.theme.has_theme_json()
    }

    /// `get_fields_to_translate()` (deprecated in 5.9).
    pub fn get_fields_to_translate(&self) -> Value {
        self.api.deps().diagnostics.deprecated_function(
            "WP_Theme_JSON_Resolver::get_fields_to_translate",
            "5.9.0",
            "",
        );
        Value::Array(Vec::new())
    }

    /// `clean_cached_data()`.
    pub fn clean_cached_data(&self) {
        *self.state.borrow_mut() = State::fresh();
    }

    /// A new request: WordPress's statics start empty.
    pub fn new_request(&self) {
        self.clean_cached_data();
        self.files.clear();
    }

    /// `get_style_variations( $scope )`.
    pub fn get_style_variations(&self, scope: &str) -> Value {
        let theme = &*self.deps.theme;
        let found =
            variations::variation_files(&theme.stylesheet_directory(), &theme.template_directory());
        let mut out = Vec::new();
        for path in found {
            let decoded = self.read_json_file(&path);
            if !variations::has_scope(&decoded, scope) {
                continue;
            }
            let translated = self.translate(decoded, &theme.text_domain());
            let mut variation = self
                .api
                .get_raw_data(&self.api.construct(translated, b"theme"));
            if array_path::get(&variation, &["title"]).is_none_or(empty) {
                let base = path.rsplit('/').next().unwrap_or(&path);
                let title = base.strip_suffix(".json").unwrap_or(base);
                array_path::insert(
                    &mut variation,
                    Key::Str("title".into()),
                    Value::Str(title.to_owned()),
                );
            }
            out.push((Key::Int(out.len() as i64), variation));
        }
        Value::Array(out)
    }

    /// `get_resolved_theme_uris( $theme_json )`.
    pub fn get_resolved_theme_uris(&self, theme_json: Option<&ThemeJson>) -> Value {
        let Some(doc) = theme_json else {
            return Value::Array(Vec::new());
        };
        let data = self.api.get_raw_data(doc);
        let mut uris = Vec::new();
        let mut add = |url: &str, target: String| {
            let theme = &*self.deps.theme;
            let src = url.replacen("file:./", "", usize::MAX);
            let mut entry = vec![
                (Key::Str("name".into()), Value::Str(url.to_owned())),
                (
                    Key::Str("href".into()),
                    Value::Str(theme.theme_file_uri(&src)),
                ),
                (Key::Str("target".into()), Value::Str(target)),
            ];
            let file_type = theme.file_type(url);
            if !matches!(file_type, Value::Null) {
                entry.push((Key::Str("type".into()), file_type));
            }
            uris.push((Key::Int(uris.len() as i64), Value::Array(entry)));
        };
        if let Some(Value::Str(url)) =
            array_path::get(&data, &["styles", "background", "backgroundImage", "url"])
            && url.starts_with("file:./")
        {
            add(url, "styles.background.backgroundImage.url".to_owned());
        }
        if let Some(blocks) = array_path::get(&data, &["styles", "blocks"]).filter(|b| !empty(b)) {
            for (name, styles) in entries(blocks) {
                let name = key_str(name);
                if let Some(Value::Str(url)) =
                    array_path::get(styles, &["background", "backgroundImage", "url"])
                    && url.starts_with("file:./")
                {
                    add(
                        url,
                        format!("styles.blocks.{name}.background.backgroundImage.url"),
                    );
                }
            }
        }
        Value::Array(uris)
    }

    /// `resolve_theme_file_uris( $theme_json )`.
    pub fn resolve_theme_file_uris(&self, theme_json: &ThemeJson) -> ThemeJson {
        let uris = self.get_resolved_theme_uris(Some(theme_json));
        if empty(&uris) {
            return theme_json.clone();
        }
        let mut data = self.api.get_raw_data(theme_json);
        for (_, uri) in entries(&uris) {
            let target = array_path::get(uri, &["target"])
                .map(|t| String::from_utf8_lossy(&crate::php::to_php_string(t)).into_owned())
                .unwrap_or_default();
            let path: Vec<&str> = target.split('.').collect();
            let href = array_path::get(uri, &["href"])
                .cloned()
                .unwrap_or(Value::Null);
            array_path::set(&mut data, &path, href);
        }
        self.api.construct(data, b"theme")
    }
}

/// `remove_json_comments( $input_array )`: `//` keys dropped at every level.
pub fn remove_json_comments(input: Value) -> Value {
    let Value::Array(entries) = input else {
        return input;
    };
    Value::Array(
        entries
            .into_iter()
            .filter(|(k, _)| *k != Key::Str("//".into()))
            .map(|(k, v)| {
                (
                    k,
                    if matches!(v, Value::Array(_)) {
                        remove_json_comments(v)
                    } else {
                        v
                    },
                )
            })
            .collect(),
    )
}
