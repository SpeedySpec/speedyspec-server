//! WordPress's translations in Rust: translation files and their lookup index, plural forms, the translation
//! controller, `WP_Locale`'s strings, the locale switcher, script translations, and the `I18nApi` facade that fires WordPress's hooks.

mod controller;
mod file;
mod gettext;
mod locale;
mod plural;
mod script;
mod switcher;

pub use controller::{FileHandle, FileRef, PhpKey, TranslationController, cached_file, php_key};
pub use file::TranslationFile;
pub use gettext::{DomainFilterNames, I18nApi, I18nError, Translated, bytes_value};
pub use locale::{LOCALE_STRINGS, LocaleData, TEXT_DIRECTION, WORD_COUNT_TYPE, locale_strings};
pub use plural::{PluralError, PluralRule};
pub use script::{Relative, ScriptFiles, json_filename, relative_path};
pub use switcher::LocaleSwitcher;
