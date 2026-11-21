//! The locale's values and the port its strings are translated through.

use std::fmt;

/// A PHP-shaped value, as the Hook API carries it: what a translation (after its filters) may be.
pub use speedyspec_hooks::Value as PhpValue;

/// A failure in an adapter behind one of this crate's ports (a PHP callback that threw, for example).
#[derive(Debug)]
pub struct PortError(pub Box<dyn std::error::Error>);

impl PortError {
    /// An error with only a message.
    pub fn message(text: impl Into<String>) -> PortError {
        PortError(text.into().into())
    }
}

impl fmt::Display for PortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for PortError {}

/// How words are counted in a locale, as `WP_Locale::get_word_count_type()` answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordCountType {
    /// `words`.
    Words,
    /// `characters_excluding_spaces`.
    CharactersExcludingSpaces,
    /// `characters_including_spaces`.
    CharactersIncludingSpaces,
}

impl WordCountType {
    /// The type a translation of `words` (context `Word count type. Do not translate!`) names: anything but the two
    /// character types means words, as WordPress decides.
    pub fn from_translation(translation: &[u8]) -> WordCountType {
        match translation {
            b"characters_excluding_spaces" => WordCountType::CharactersExcludingSpaces,
            b"characters_including_spaces" => WordCountType::CharactersIncludingSpaces,
            _ => WordCountType::Words,
        }
    }

    /// WordPress's name for the type.
    pub fn as_str(&self) -> &'static str {
        match self {
            WordCountType::Words => "words",
            WordCountType::CharactersExcludingSpaces => "characters_excluding_spaces",
            WordCountType::CharactersIncludingSpaces => "characters_including_spaces",
        }
    }
}

/// Translates the default domain's strings, as `__()` and `_x()` do, filters included.
pub trait LocaleStrings {
    /// `__( $text )`.
    fn translate(&self, text: &str) -> Result<PhpValue, PortError>;
    /// `_x( $text, $context )`.
    fn translate_with_context(&self, text: &str, context: &str) -> Result<PhpValue, PortError>;
}

/// What switching a locale does besides keeping the stack, as `WP_Locale_Switcher` does it. Locales and user ids are
/// PHP values: WordPress passes them through unchecked.
pub trait SwitchEffects {
    /// `determine_locale()`, filters included.
    fn determine_locale(&self) -> Result<PhpValue, PortError>;
    /// `get_user_locale( $user )`.
    fn user_locale(&self, user: &PhpValue) -> Result<PhpValue, PortError>;
    /// The switcher's private `change_locale()`: reload the loaded domains' translations, a new `$wp_locale`, the
    /// translation controller's locale, PHPMailer's language, then the `change_locale` action.
    fn change_locale(&self, locale: &PhpValue) -> Result<(), PortError>;
    /// The `switch_locale` action.
    fn switched(&self, locale: &PhpValue, user: &PhpValue) -> Result<(), PortError>;
    /// The `restore_previous_locale` action.
    fn restored(&self, locale: &PhpValue, previous: &PhpValue) -> Result<(), PortError>;
}

/// PHP's truthiness, for the values a locale or user id may be.
pub fn php_truthy(value: &PhpValue) -> bool {
    match value {
        PhpValue::Null => false,
        PhpValue::Bool(b) => *b,
        PhpValue::Int(n) => *n != 0,
        PhpValue::Float(f) => *f != 0.0,
        PhpValue::Str(s) => !(s.is_empty() || s == "0"),
        PhpValue::Bytes(b) => !(b.is_empty() || b.as_slice() == b"0"),
        PhpValue::Array(items) => !items.is_empty(),
        PhpValue::Opaque(_) => true,
    }
}
