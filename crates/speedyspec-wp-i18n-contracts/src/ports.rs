//! The ports the translations domain depends on.

use std::any::Any;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use speedyspec_hooks::Value;

use crate::values::{FileIdentity, RawTranslations, ReadError};

/// A failure from code behind a port, carried unchanged so a language bridge can rethrow it (a PHP exception).
#[derive(Clone, Debug)]
pub struct PortError {
    pub message: String,
    pub payload: Option<Rc<dyn Any>>,
}

/// Reads a translation file into [`RawTranslations`].
pub trait TranslationFileReader {
    fn read(&self, path: &Path) -> Result<RawTranslations, ReadError>;
}

/// The file system questions `WP_Translation_Controller` and `WP_Textdomain_Registry` ask.
pub trait FileProbe {
    /// `realpath()`.
    fn realpath(&self, path: &Path) -> Option<PathBuf>;
    /// `is_readable()`.
    fn is_readable(&self, path: &Path) -> bool;
    /// The file's identity, `None` when it does not exist.
    fn identity(&self, path: &Path) -> Option<FileIdentity>;
    /// `glob( "$dir$pattern" )`, where `pattern` may hold `*` (never `/`), sorted as `glob()` sorts.
    fn glob(&self, dir: &Path, pattern: &str) -> Vec<PathBuf>;
}

/// The loaded translation files a worker keeps across requests, one per identity.
pub trait TranslationFileCache<F> {
    fn get(&self, identity: &FileIdentity) -> Option<Rc<F>>;
    fn put(&self, identity: FileIdentity, file: Rc<F>);
}

/// The site's locale settings, as `get_locale()` reads them.
pub trait SiteLocaleSource {
    /// The `$locale` global, if set.
    fn global_locale(&self) -> Option<String>;
    /// Sets the `$locale` global, as `get_locale()` does.
    fn set_global_locale(&self, locale: &str);
    /// The `$wp_local_package` global, if set.
    fn local_package(&self) -> Option<String>;
    /// The `WPLANG` constant, if defined.
    fn wplang_constant(&self) -> Option<String>;
    fn is_multisite(&self) -> bool;
    fn installing(&self) -> bool;
    /// `get_option( $name )`; `None` where WordPress gets `false`.
    fn option(&self, name: &str) -> Result<Option<Value>, PortError>;
    /// `get_site_option( $name )`; `None` where WordPress gets `false`.
    fn site_option(&self, name: &str) -> Result<Option<Value>, PortError>;
}

/// Whose locale `get_user_locale()` asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserRef {
    Current,
    Id(u64),
}

/// A user's `locale` setting.
pub trait UserLocaleSource {
    /// The user's locale; `None` when there is no such user or the setting is empty.
    fn user_locale(&self, user: UserRef) -> Result<Option<String>, PortError>;
}

/// What `determine_locale()` reads from the request.
pub trait RequestContext {
    /// The `$pagenow` global.
    fn pagenow(&self) -> Option<String>;
    /// `$_GET[ $key ]`, when it is set and not empty.
    fn get(&self, key: &str) -> Option<Value>;
    /// `$_COOKIE[ $key ]`, when it is set and not empty.
    fn cookie(&self, key: &str) -> Option<Value>;
    /// `$_REQUEST[ $key ]`, when it is set and not empty.
    fn request(&self, key: &str) -> Option<Value>;
    fn is_admin(&self) -> bool;
    fn is_json_request(&self) -> bool;
    fn installing(&self) -> bool;
    /// The `$wp_local_package` global, if set.
    fn local_package(&self) -> Option<String>;
}

/// WordPress's notices for misuse.
pub trait Deprecations {
    /// `_doing_it_wrong( $function, $message, $version )`.
    fn doing_it_wrong(&self, function: &str, message: &str, version: &str);
}

/// The active theme's directories.
pub trait ThemeDirectories {
    fn template_directory(&self) -> PathBuf;
    fn stylesheet_directory(&self) -> PathBuf;
}

/// Where translation files live.
pub trait LanguageDirectories {
    /// `WP_LANG_DIR`, when defined.
    fn lang_dir(&self) -> Option<PathBuf>;
    /// `WP_PLUGIN_DIR`.
    fn plugin_dir(&self) -> PathBuf;
    /// `WPMU_PLUGIN_DIR`.
    fn mu_plugin_dir(&self) -> PathBuf;
}

/// `sanitize_locale_name()` (formatting.php).
pub trait LocaleNameSanitizer {
    fn sanitize_locale_name(&self, name: &Value) -> Result<Value, PortError>;
}

/// `esc_html()` and `esc_attr()` (formatting.php), filters included.
pub trait Escaper {
    fn esc_html(&self, text: Value) -> Result<Value, PortError>;
    fn esc_attr(&self, text: Value) -> Result<Value, PortError>;
}
