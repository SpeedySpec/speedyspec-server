//! The fixed adapters answer as a fresh WordPress install does, and diagnostics are kept in order.

use speedyspec_wp_text_contracts::{
    Diagnostics, ErrorLevel, Locale, SiteSettings, Translator, WordCountType,
};
use speedyspec_wp_text_memory::{
    Diagnostic, EnglishTranslator, FixedLocale, FixedSiteSettings, RecordingDiagnostics,
};

#[test]
fn fixed_site_settings_default_to_wordpress_install_defaults() {
    let settings = FixedSiteSettings::default();
    assert_eq!(settings.blog_charset().unwrap(), "UTF-8");
    assert!(settings.use_smilies().unwrap());
    assert!(!settings.use_balance_tags().unwrap());
    assert_eq!(settings.word_count_type().unwrap(), WordCountType::Words);
}

#[test]
fn english_translator_returns_input() {
    let translator = EnglishTranslator;
    assert_eq!(translator.translate("&#8220;", "default").unwrap(), "&#8220;");
    assert_eq!(
        translator
            .translate_with_context("&#8220;", "opening curly double quote", "default")
            .unwrap(),
        "&#8220;"
    );
}

#[test]
fn fixed_locale_defaults_to_en_us() {
    assert_eq!(FixedLocale::default().locale().unwrap(), "en_US");
}

#[test]
fn recording_diagnostics_keeps_calls_in_order() {
    let diagnostics = RecordingDiagnostics::default();
    diagnostics
        .deprecated_function("seems_utf8", "6.9.0", "wp_is_valid_utf8")
        .unwrap();
    diagnostics
        .trigger_error("wp_strip_all_tags", "msg", ErrorLevel::Warning)
        .unwrap();
    assert_eq!(
        diagnostics.taken(),
        vec![
            Diagnostic::DeprecatedFunction {
                function: "seems_utf8".into(),
                version: "6.9.0".into(),
                replacement: "wp_is_valid_utf8".into(),
            },
            Diagnostic::TriggeredError {
                function: "wp_strip_all_tags".into(),
                message: "msg".into(),
                level: ErrorLevel::Warning,
            },
        ]
    );
    assert!(diagnostics.taken().is_empty());
}
