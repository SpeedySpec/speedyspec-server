//! The outside facts formatting reads.

use std::rc::Rc;

use crate::TextError;

/// How `wp_trim_words()` counts, as `wp_get_word_count_type()` answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordCountType {
    /// `words`.
    Words,
    /// `characters_excluding_spaces`.
    CharactersExcludingSpaces,
    /// `characters_including_spaces`.
    CharactersIncludingSpaces,
}

/// The site options formatting depends on.
pub trait SiteSettings {
    /// The `blog_charset` option, which decides how escaping treats bytes.
    fn blog_charset(&self) -> Result<String, TextError>;
    /// The `use_smilies` option, which turns `convert_smilies()` on.
    fn use_smilies(&self) -> Result<bool, TextError>;
    /// The `use_balanceTags` option, which turns `balanceTags()` on without `$force`.
    fn use_balance_tags(&self) -> Result<bool, TextError>;
    /// How words are counted in the current locale.
    fn word_count_type(&self) -> Result<WordCountType, TextError>;
}

/// Translations, as `__()` and `_x()` give them.
///
/// An implementation over the i18n port goes through its facade, so the `gettext` and `gettext_with_context`
/// filters fire as they do in WordPress.
pub trait Translator {
    fn translate(&self, text: &str, domain: &str) -> Result<String, TextError>;
    fn translate_with_context(
        &self,
        text: &str,
        context: &str,
        domain: &str,
    ) -> Result<String, TextError>;
}

/// The current locale, as `get_locale()` answers it.
pub trait Locale {
    fn locale(&self) -> Result<String, TextError>;
}

/// The level of a triggered error, as `wp_trigger_error()` takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorLevel {
    /// `E_USER_NOTICE`.
    Notice,
    /// `E_USER_WARNING`.
    Warning,
    /// `E_USER_DEPRECATED`.
    Deprecated,
    /// `E_USER_ERROR`.
    Error,
}

/// Where formatting reports misuse, as WordPress's `_deprecated_function()`, `_deprecated_argument()` and
/// `wp_trigger_error()` do.
///
/// WordPress's own tests expect these reports, and Rust callers should see them too.
pub trait Diagnostics {
    fn deprecated_function(
        &self,
        function: &str,
        version: &str,
        replacement: &str,
    ) -> Result<(), TextError>;
    fn deprecated_argument(
        &self,
        function: &str,
        version: &str,
        message: &str,
    ) -> Result<(), TextError>;
    fn trigger_error(
        &self,
        function: &str,
        message: &str,
        level: ErrorLevel,
    ) -> Result<(), TextError>;
}

/// `(word, replacement)` pairs, in order, as PHP's `$wp_cockneyreplace` array holds them.
pub type ReplacementPairs = Vec<(Vec<u8>, Vec<u8>)>;

/// `$wp_cockneyreplace`: a plugin's own list of words `wptexturize()` replaces, in place of the translated one.
pub trait TexturizeOverrides {
    /// The pairs, in the array's order; `None` when the global is not set.
    fn cockney(&self) -> Result<Option<ReplacementPairs>, TextError>;
}

/// `(extension pattern, MIME type)` pairs, in order, as `wp_get_mime_types()` returns them (`jpg|jpeg|jpe`).
pub type MimePairs = Vec<(Vec<u8>, Vec<u8>)>;

/// The MIME lists `sanitize_file_name()` reads.
///
/// The PHP adapter calls WordPress's functions, so their `mime_types` and `upload_mimes` filters fire there.
pub trait MimeTypes {
    /// `wp_get_mime_types()`, shared so a lookup copies nothing.
    fn mime_types(&self) -> Result<Rc<MimePairs>, TextError>;
    /// `get_allowed_mime_types()` for the current user.
    fn allowed_mime_types(&self) -> Result<Rc<MimePairs>, TextError>;
}

/// PHP's `rand( $min, $max )`, which `antispambot()` uses to choose how to write each character.
pub trait RandomSource {
    /// A number from `min` to `max`, both included.
    fn rand(&self, min: i64, max: i64) -> Result<i64, TextError>;
}

/// The smileys `convert_smilies()` replaces: `$wp_smiliessearch`, `$wpsmiliestrans` and where the images are.
pub trait Smilies {
    /// `$wp_smiliessearch`, or `None` when it is empty (smilies off, or no table).
    fn search(&self) -> Result<Option<Vec<u8>>, TextError>;
    /// The keys of `$wpsmiliestrans`, in order (a fast matcher is used only when they rebuild the search).
    fn smileys(&self) -> Result<Vec<Vec<u8>>, TextError>;
    /// `$wpsmiliestrans[ $smiley ]`: the image file (or the text) for a smiley; `None` when it has none.
    fn image(&self, smiley: &[u8]) -> Result<Option<Vec<u8>>, TextError>;
    /// `includes_url( $path )`.
    fn includes_url(&self, path: &[u8]) -> Result<Vec<u8>, TextError>;
    /// `site_url()`.
    fn site_url(&self) -> Result<Vec<u8>, TextError>;
}

/// Whether a link points at this site, as `wp_is_internal_link()` answers (its `wp_internal_hosts` filter fires in
/// the PHP adapter).
pub trait LinkContext {
    /// `wp_is_internal_link( $url )`.
    fn is_internal_link(&self, url: &[u8]) -> Result<bool, TextError>;
}
