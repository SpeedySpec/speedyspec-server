//! WordPress's translations in Rust: translation files and their lookup index, plural forms, the translation
//! controller, and the `I18nApi` facade that fires WordPress's hooks.

mod controller;
mod file;
mod gettext;
mod plural;

pub use controller::{FileHandle, FileRef, PhpKey, TranslationController, php_key};
pub use file::TranslationFile;
pub use gettext::{DomainFilterNames, I18nApi, I18nError, Translated, bytes_value};
pub use plural::{PluralError, PluralRule};
