//! WordPress's translations in Rust: translation files and their lookup index, plural forms, the translation
//! controller, and the `I18nApi` facade that fires WordPress's hooks.

mod file;
mod plural;

pub use file::TranslationFile;
pub use plural::{PluralError, PluralRule};
