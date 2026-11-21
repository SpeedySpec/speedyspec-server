//! The ports of WordPress's formatting functions.
//!
//! Formatting reads a few outside facts: the site's charset and settings, translations, the locale, and where to
//! report deprecations. Each comes through a port here, so the formatting core stays pure and an implementation (in
//! memory for tests, over PHP in the embedded runtime, over the Rust i18n port later) can be swapped wholly or in
//! part. This crate depends on nothing of ours.

mod error;
mod ports;

pub use error::TextError;
pub use ports::{Diagnostics, ErrorLevel, Locale, ReplacementPairs, SiteSettings, TexturizeOverrides, Translator, WordCountType};
