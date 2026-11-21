//! WordPress's translations in Rust: translation files and their lookup index, plural forms, the translation
//! controller, and the `I18nApi` facade that fires WordPress's hooks.

mod plural;

pub use plural::{PluralError, PluralRule};
