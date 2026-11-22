//! In-memory adapters for the resolver's ports: a theme over directories, no data filters, global styles posts and
//! a block style variation registrar over `MemoryBlockMetadata`.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::ports::{
    ActiveTheme, BlockStyleVariationRegistrar, FilteredLayer, GlobalStylesPost, LayerFilters,
    WpCache,
};
use speedyspec_wp_theme_json_contracts::{Origin, ThemeJsonError};

use super::MemoryBlockMetadata;
use crate::array_path;
use crate::php::to_php_string;
use crate::php_value::{empty, entries};
use crate::presets::to_kebab_case;

/// A theme over a directory (and its parent's), reading `style.css` headers as WordPress does.
pub struct MemoryActiveTheme {
    /// The theme's directory.
    pub dir: String,
    /// The parent theme's directory, for a child theme.
    pub template_dir: Option<String>,
    /// `add_theme_support()` features.
    pub supports: RefCell<Vec<String>>,
    /// `get_classic_theme_supports_block_editor_settings()`.
    pub classic_settings: RefCell<Value>,
    /// `wp_is_development_mode( 'theme' )`.
    pub development_mode: bool,
}

fn header(dir: &str, name: &str) -> Value {
    let Ok(css) = std::fs::read_to_string(format!("{dir}/style.css")) else {
        return Value::Bool(false);
    };
    for line in css.lines() {
        let trimmed = line.trim_start_matches([' ', '\t', '/', '*', '#', '@']);
        if trimmed.len() > name.len()
            && trimmed[..name.len()].eq_ignore_ascii_case(name)
            && trimmed[name.len()..].starts_with(':')
        {
            let value = trimmed[name.len() + 1..].trim();
            let value = value.split("*/").next().unwrap_or(value).trim_end();
            return Value::Str(value.to_owned());
        }
    }
    Value::Bool(false)
}

impl MemoryActiveTheme {
    /// The theme in `dir`, a child of the theme in `template_dir` when given.
    pub fn new(dir: &str, template_dir: Option<&str>) -> MemoryActiveTheme {
        MemoryActiveTheme {
            dir: dir.to_owned(),
            template_dir: template_dir.map(str::to_owned),
            supports: RefCell::new(Vec::new()),
            classic_settings: RefCell::new(Value::Array(Vec::new())),
            development_mode: false,
        }
    }

    fn template(&self) -> String {
        self.template_dir
            .clone()
            .unwrap_or_else(|| self.dir.clone())
    }
}

impl ActiveTheme for MemoryActiveTheme {
    fn supports(&self, feature: &str) -> bool {
        self.supports.borrow().iter().any(|f| f == feature)
    }
    fn stylesheet(&self) -> String {
        self.dir.rsplit('/').next().unwrap_or_default().to_owned()
    }
    fn stylesheet_directory(&self) -> String {
        self.dir.clone()
    }
    fn template_directory(&self) -> String {
        self.template()
    }
    fn theme_json_path(&self) -> String {
        let child = format!("{}/theme.json", self.dir);
        if self.dir != self.template() && std::path::Path::new(&child).exists() {
            child
        } else {
            format!("{}/theme.json", self.template())
        }
    }
    fn parent_theme_json_path(&self) -> Option<String> {
        self.template_dir
            .as_ref()
            .map(|t| format!("{t}/theme.json"))
    }
    fn text_domain(&self) -> Value {
        header(&self.dir, "Text Domain")
    }
    fn parent_text_domain(&self) -> Value {
        self.template_dir
            .as_ref()
            .map_or(Value::Bool(false), |t| header(t, "Text Domain"))
    }
    fn has_theme_json(&self) -> bool {
        std::path::Path::new(&self.theme_json_path()).exists()
    }
    fn is_block_theme(&self) -> bool {
        let t = self.template();
        std::path::Path::new(&format!("{t}/templates/index.html")).exists()
            || std::path::Path::new(&format!("{t}/block-templates/index.html")).exists()
    }
    fn is_development_mode(&self) -> bool {
        self.development_mode
    }
    fn classic_editor_settings(&self) -> Value {
        self.classic_settings.borrow().clone()
    }
    fn theme_file_uri(&self, file: &str) -> String {
        format!(
            "https://example.org/wp-content/themes/{}/{}",
            self.stylesheet(),
            file.trim_start_matches('/')
        )
    }
    fn file_type(&self, file: &str) -> Value {
        let ext = file
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let mime = match ext.as_str() {
            "jpg" | "jpeg" | "jpe" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "avif" => "image/avif",
            _ => return Value::Bool(false),
        };
        Value::Str(mime.to_owned())
    }
}

/// No data filters (a Null Object): every layer is built from its data as it is.
pub struct NoLayerFilters;

impl LayerFilters for NoLayerFilters {
    fn apply(
        &self,
        _hook: &str,
        _origin: Origin,
        config: Value,
    ) -> Result<FilteredLayer, ThemeJsonError> {
        Ok(FilteredLayer::Config(config))
    }
}

/// Global styles posts held in memory, newest last.
#[derive(Default)]
pub struct MemoryGlobalStylesPosts {
    /// Each post: `ID`, `stylesheet`, `post_status`, `post_content`, … .
    pub posts: RefCell<Vec<Value>>,
}

impl MemoryGlobalStylesPosts {
    /// Saves the user's styles for a theme.
    pub fn add(&self, stylesheet: &str, content: &str, status: &str) {
        let id = 100 + self.posts.borrow().len() as i64;
        self.posts.borrow_mut().push(Value::Array(vec![
            (Key::Str("ID".into()), Value::Int(id)),
            (Key::Str("stylesheet".into()), Value::Str(stylesheet.into())),
            (Key::Str("post_status".into()), Value::Str(status.into())),
            (Key::Str("post_content".into()), Value::Str(content.into())),
            (
                Key::Str("post_type".into()),
                Value::Str("wp_global_styles".into()),
            ),
        ]));
    }
}

impl GlobalStylesPost for MemoryGlobalStylesPosts {
    fn find(&self, stylesheet: &str, create: bool, statuses: &Value) -> Value {
        let statuses: Vec<Vec<u8>> = match statuses {
            Value::Array(items) => items.iter().map(|(_, v)| to_php_string(v)).collect(),
            other => vec![to_php_string(other)],
        };
        let found = self
            .posts
            .borrow()
            .iter()
            .rev()
            .find(|p| {
                array_path::get(p, &["stylesheet"]) == Some(&Value::Str(stylesheet.into()))
                    && array_path::get(p, &["post_status"])
                        .is_some_and(|s| statuses.contains(&to_php_string(s)))
            })
            .cloned();
        if let Some(post) = found {
            return without_stylesheet(post);
        }
        if create {
            self.add(
                stylesheet,
                "{\"version\": 3, \"isGlobalStylesUserThemeJSON\": true }",
                "publish",
            );
            let post = self.posts.borrow().last().cloned().unwrap_or(Value::Null);
            return without_stylesheet(post);
        }
        Value::Array(Vec::new())
    }
}

fn without_stylesheet(post: Value) -> Value {
    match post {
        Value::Array(fields) => Value::Array(
            fields
                .into_iter()
                .filter(|(k, _)| *k != Key::Str("stylesheet".into()))
                .collect(),
        ),
        other => other,
    }
}

/// `wp_register_block_style_variations_from_theme_json_partials()` over `MemoryBlockMetadata`.
pub struct MemoryVariationRegistrar(pub Rc<MemoryBlockMetadata>);

impl BlockStyleVariationRegistrar for MemoryVariationRegistrar {
    fn register(&self, variations: &Value) {
        for (_, variation) in entries(variations) {
            let (Some(block_types), Some(_)) = (
                array_path::get(variation, &["blockTypes"]).filter(|v| !empty(v)),
                array_path::get(variation, &["styles"]).filter(|v| !empty(v)),
            ) else {
                continue;
            };
            let title = array_path::get(variation, &["title"]).cloned();
            let name =
                match array_path::get(variation, &["slug"]).filter(|v| !matches!(v, Value::Null)) {
                    Some(slug) => String::from_utf8_lossy(&to_php_string(slug)).into_owned(),
                    None => to_kebab_case(&to_php_string(title.as_ref().unwrap_or(&Value::Null))),
                };
            let label = title
                .filter(|t| !matches!(t, Value::Null))
                .unwrap_or(Value::Str(name.clone()));
            for (_, block_type) in entries(block_types) {
                let block_type = String::from_utf8_lossy(&to_php_string(block_type)).into_owned();
                if !self.0.styles_for(&block_type).contains(&name) {
                    self.0.register_style(
                        &block_type,
                        Value::Array(vec![
                            (Key::Str("name".into()), Value::Str(name.clone())),
                            (Key::Str("label".into()), label.clone()),
                        ]),
                    );
                }
            }
        }
    }
}

/// WordPress's object cache, in memory.
#[derive(Default)]
pub struct MemoryWpCache {
    /// Each entry: group, key, value.
    pub entries: RefCell<Vec<(String, String, Value)>>,
}

impl WpCache for MemoryWpCache {
    fn get(&self, key: &str, group: &str) -> Option<Value> {
        self.entries
            .borrow()
            .iter()
            .find(|(g, k, _)| g == group && k == key)
            .map(|(_, _, v)| v.clone())
    }
    fn set(&self, key: &str, value: Value, group: &str) {
        let mut entries = self.entries.borrow_mut();
        match entries.iter_mut().find(|(g, k, _)| g == group && k == key) {
            Some(slot) => slot.2 = value,
            None => entries.push((group.to_owned(), key.to_owned(), value)),
        }
    }
    fn delete(&self, key: &str, group: &str) {
        self.entries
            .borrow_mut()
            .retain(|(g, k, _)| !(g == group && k == key));
    }
}
