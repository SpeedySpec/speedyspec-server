//! Fixed and in-memory adapters for the text ports.
//!
//! They answer as a fresh WordPress install in English does, for tests and for Rust callers that run formatting
//! with no PHP and no options store.

use std::cell::RefCell;

use speedyspec_wp_text_contracts::{
    Diagnostics, ErrorLevel, Locale, SiteSettings, TextError, TexturizeOverrides, Translator, WordCountType,
};

/// Site settings given up front.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixedSiteSettings {
    pub blog_charset: String,
    pub use_smilies: bool,
    pub use_balance_tags: bool,
    pub word_count_type: WordCountType,
}

impl Default for FixedSiteSettings {
    /// WordPress's install defaults.
    fn default() -> FixedSiteSettings {
        FixedSiteSettings {
            blog_charset: "UTF-8".into(),
            use_smilies: true,
            use_balance_tags: false,
            word_count_type: WordCountType::Words,
        }
    }
}

impl SiteSettings for FixedSiteSettings {
    fn blog_charset(&self) -> Result<String, TextError> {
        Ok(self.blog_charset.clone())
    }

    fn use_smilies(&self) -> Result<bool, TextError> {
        Ok(self.use_smilies)
    }

    fn use_balance_tags(&self) -> Result<bool, TextError> {
        Ok(self.use_balance_tags)
    }

    fn word_count_type(&self) -> Result<WordCountType, TextError> {
        Ok(self.word_count_type)
    }
}

/// No translation: every string is its own English.
#[derive(Clone, Copy, Debug, Default)]
pub struct EnglishTranslator;

impl Translator for EnglishTranslator {
    fn translate(&self, text: &str, _domain: &str) -> Result<String, TextError> {
        Ok(text.to_string())
    }

    fn translate_with_context(
        &self,
        text: &str,
        _context: &str,
        _domain: &str,
    ) -> Result<String, TextError> {
        Ok(text.to_string())
    }
}

/// A locale given up front.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixedLocale(pub String);

impl Default for FixedLocale {
    fn default() -> FixedLocale {
        FixedLocale("en_US".into())
    }
}

impl Locale for FixedLocale {
    fn locale(&self) -> Result<String, TextError> {
        Ok(self.0.clone())
    }
}

/// One report a [`RecordingDiagnostics`] kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Diagnostic {
    DeprecatedFunction {
        function: String,
        version: String,
        replacement: String,
    },
    DeprecatedArgument {
        function: String,
        version: String,
        message: String,
    },
    TriggeredError {
        function: String,
        message: String,
        level: ErrorLevel,
    },
}

/// Keeps every report, in order, so tests can check what formatting reported.
#[derive(Debug, Default)]
pub struct RecordingDiagnostics {
    kept: RefCell<Vec<Diagnostic>>,
}

impl RecordingDiagnostics {
    /// The reports kept so far, emptying the record.
    pub fn taken(&self) -> Vec<Diagnostic> {
        self.kept.take()
    }

    fn keep(&self, diagnostic: Diagnostic) -> Result<(), TextError> {
        self.kept.borrow_mut().push(diagnostic);
        Ok(())
    }
}

impl Diagnostics for RecordingDiagnostics {
    fn deprecated_function(
        &self,
        function: &str,
        version: &str,
        replacement: &str,
    ) -> Result<(), TextError> {
        self.keep(Diagnostic::DeprecatedFunction {
            function: function.into(),
            version: version.into(),
            replacement: replacement.into(),
        })
    }

    fn deprecated_argument(
        &self,
        function: &str,
        version: &str,
        message: &str,
    ) -> Result<(), TextError> {
        self.keep(Diagnostic::DeprecatedArgument {
            function: function.into(),
            version: version.into(),
            message: message.into(),
        })
    }

    fn trigger_error(
        &self,
        function: &str,
        message: &str,
        level: ErrorLevel,
    ) -> Result<(), TextError> {
        self.keep(Diagnostic::TriggeredError {
            function: function.into(),
            message: message.into(),
            level,
        })
    }
}

/// A fixed `$wp_cockneyreplace`: `None` (the default) is WordPress with no plugin setting it.
#[derive(Clone, Debug, Default)]
pub struct FixedTexturizeOverrides(pub Option<Vec<(Vec<u8>, Vec<u8>)>>);

impl TexturizeOverrides for FixedTexturizeOverrides {
    fn cockney(&self) -> Result<Option<Vec<(Vec<u8>, Vec<u8>)>>, TextError> {
        Ok(self.0.clone())
    }
}

/// A fixed list of registered shortcode names, in registration order, for formatting without a shortcode registry.
#[derive(Clone, Debug, Default)]
pub struct FixedShortcodeTags(pub Vec<speedyspec_wp_shortcode_contracts::TagName>);

impl speedyspec_wp_shortcode_contracts::ShortcodeTagNames for FixedShortcodeTags {
    fn tag_names(&self) -> Vec<speedyspec_wp_shortcode_contracts::TagName> {
        self.0.clone()
    }

    /// A hash of the names, so equal lists have equal generations.
    fn generation(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for name in &self.0 {
            name.as_bytes().hash(&mut hasher);
        }
        hasher.finish()
    }
}
