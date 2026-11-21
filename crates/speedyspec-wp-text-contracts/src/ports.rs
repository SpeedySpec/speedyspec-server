//! The outside facts formatting reads.

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
