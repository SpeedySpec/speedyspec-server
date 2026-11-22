//! The text ports over WordPress's PHP (Adapter): each answer comes from the PHP function or global WordPress itself
//! uses, so its filters (`gettext`, `locale`, `mime_types`, `upload_mimes`, `wp_internal_hosts`, ...) fire in PHP.

use std::any::Any;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;

use speedyspec_php::{CallFailure, PhpError};
use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_shortcode_contracts::{ShortcodeTagNames, TagName};
use speedyspec_wp_text::strings::to_php_string;
use speedyspec_wp_text_contracts::{
    Diagnostics, ErrorLevel, LinkContext, Locale, MimePairs, MimeTypes, RandomSource, ReplacementPairs, SiteSettings,
    Smilies, TextError, TexturizeOverrides, Translator, WordCountType,
};

/// Marks a failure raised because PHP is unwinding (a fatal error or `exit()`): nothing is rethrown.
pub struct Unwinding;

/// A failed PHP call as a text error that carries the thrown object (or the unwinding mark) as its payload.
pub(crate) fn call_error(failure: CallFailure) -> TextError {
    let message = match &failure.error {
        PhpError::Exception { message, .. } => message.clone(),
        other => other.to_string(),
    };
    let payload: Option<Rc<dyn Any>> = match (failure.exception, &failure.error) {
        (Some(exception), _) => Some(Rc::new(exception)),
        (None, PhpError::Fatal { .. } | PhpError::Exit(_)) => Some(Rc::new(Unwinding)),
        (None, _) => None,
    };
    TextError { message, payload }
}

/// Calls the PHP function `name`.
pub(crate) fn call(name: &str, args: &[Value]) -> Result<Value, TextError> {
    speedyspec_php::call_catching(&Value::Str(name.to_string()), args).map_err(call_error)
}

/// A value as PHP's `(string)` makes it; an array or object gives `""`.
fn text(value: &Value) -> Vec<u8> {
    to_php_string(&value.read()).unwrap_or_default()
}

/// PHP's truthiness.
fn truthy(value: &Value) -> bool {
    match &*value.read() {
        Value::Null | Value::Bool(false) => false,
        Value::Int(i) => *i != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => !(s.is_empty() || s == "0"),
        Value::Bytes(b) => !(b.is_empty() || b == b"0"),
        Value::Array(a) => !a.is_empty(),
        _ => true,
    }
}

/// A PHP array's `(key, value)` pairs as strings, in order; anything else is empty.
fn string_pairs(value: &Value) -> Vec<(Vec<u8>, Vec<u8>)> {
    match &*value.read() {
        Value::Array(items) => items
            .iter()
            .map(|(k, v)| {
                let key = match k {
                    Key::Int(i) => i.to_string().into_bytes(),
                    Key::Str(s) => s.as_bytes().to_vec(),
                };
                (key, text(v))
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn str_arg(s: &str) -> Value {
    Value::Str(s.to_string())
}

fn bytes_arg(b: &[u8]) -> Value {
    speedyspec_wp_text::strings::bytes_value(b.to_vec())
}

/// `get_option()` for the settings formatting reads, and `wp_get_word_count_type()`.
pub struct PhpSiteSettings;

impl SiteSettings for PhpSiteSettings {
    fn blog_charset(&self) -> Result<String, TextError> {
        Ok(String::from_utf8_lossy(&text(&call("get_option", &[str_arg("blog_charset")])?)).into_owned())
    }

    fn use_smilies(&self) -> Result<bool, TextError> {
        Ok(truthy(&call("get_option", &[str_arg("use_smilies")])?))
    }

    fn use_balance_tags(&self) -> Result<bool, TextError> {
        Ok(truthy(&call("get_option", &[str_arg("use_balanceTags")])?))
    }

    fn word_count_type(&self) -> Result<WordCountType, TextError> {
        Ok(match text(&call("wp_get_word_count_type", &[])?).as_slice() {
            b"characters_excluding_spaces" => WordCountType::CharactersExcludingSpaces,
            b"characters_including_spaces" => WordCountType::CharactersIncludingSpaces,
            _ => WordCountType::Words,
        })
    }
}

/// PHP's `__()` and `_x()`, so `gettext` and `gettext_with_context` fire.
pub struct PhpTranslator;

impl Translator for PhpTranslator {
    fn translate(&self, text_in: &str, domain: &str) -> Result<String, TextError> {
        Ok(String::from_utf8_lossy(&text(&call("__", &[str_arg(text_in), str_arg(domain)])?)).into_owned())
    }

    fn translate_with_context(&self, text_in: &str, context: &str, domain: &str) -> Result<String, TextError> {
        let out = call("_x", &[str_arg(text_in), str_arg(context), str_arg(domain)])?;
        Ok(String::from_utf8_lossy(&text(&out)).into_owned())
    }
}

/// `get_locale()` (its `locale` filter fires in PHP).
pub struct PhpLocale;

impl Locale for PhpLocale {
    fn locale(&self) -> Result<String, TextError> {
        Ok(String::from_utf8_lossy(&text(&call("get_locale", &[])?)).into_owned())
    }
}

/// `_deprecated_function()`, `_deprecated_argument()` and `wp_trigger_error()`.
pub struct PhpDiagnostics;

impl Diagnostics for PhpDiagnostics {
    fn deprecated_function(&self, function: &str, version: &str, replacement: &str) -> Result<(), TextError> {
        call("_deprecated_function", &[str_arg(function), str_arg(version), str_arg(replacement)]).map(drop)
    }

    fn deprecated_argument(&self, function: &str, version: &str, message: &str) -> Result<(), TextError> {
        call("_deprecated_argument", &[str_arg(function), str_arg(version), str_arg(message)]).map(drop)
    }

    fn trigger_error(&self, function: &str, message: &str, level: ErrorLevel) -> Result<(), TextError> {
        let level = match level {
            ErrorLevel::Notice => 1024,
            ErrorLevel::Warning => 512,
            ErrorLevel::Deprecated => 16384,
            ErrorLevel::Error => 256,
        };
        call("wp_trigger_error", &[str_arg(function), str_arg(message), Value::Int(level)]).map(drop)
    }
}

/// `$wp_cockneyreplace`.
pub struct PhpTexturizeOverrides;

impl TexturizeOverrides for PhpTexturizeOverrides {
    fn cockney(&self) -> Result<Option<ReplacementPairs>, TextError> {
        Ok(speedyspec_php::global("wp_cockneyreplace").filter(|v| matches!(&*v.read(), Value::Array(_))).map(|v| string_pairs(&v)))
    }
}

/// `$shortcode_tags`' keys, in order; the generation is a hash of the key sequence, so any change to the set or its
/// order changes it.
pub struct PhpShortcodeTags;

impl PhpShortcodeTags {
    fn keys() -> Vec<Vec<u8>> {
        speedyspec_php::global_array_keys("shortcode_tags").unwrap_or_default()
    }
}

impl ShortcodeTagNames for PhpShortcodeTags {
    fn tag_names(&self) -> Vec<TagName> {
        PhpShortcodeTags::keys().into_iter().map(TagName::from_global).collect()
    }

    fn generation(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        PhpShortcodeTags::keys().hash(&mut hasher);
        hasher.finish()
    }
}

/// `wp_get_mime_types()` and `get_allowed_mime_types()` (their `mime_types` and `upload_mimes` filters fire in PHP).
pub struct PhpMimeTypes;

impl MimeTypes for PhpMimeTypes {
    fn mime_types(&self) -> Result<Rc<MimePairs>, TextError> {
        Ok(Rc::new(string_pairs(&call("wp_get_mime_types", &[])?)))
    }

    fn allowed_mime_types(&self) -> Result<Rc<MimePairs>, TextError> {
        Ok(Rc::new(string_pairs(&call("get_allowed_mime_types", &[])?)))
    }
}

/// PHP's `rand( $min, $max )`, so a seed set with `mt_srand()` applies.
pub struct PhpRandom;

impl RandomSource for PhpRandom {
    fn rand(&self, min: i64, max: i64) -> Result<i64, TextError> {
        match call("rand", &[Value::Int(min), Value::Int(max)])? {
            Value::Int(n) => Ok(n),
            other => Err(TextError::message(format!("rand() returned {other:?}"))),
        }
    }
}

/// `$wp_smiliessearch`, `$wpsmiliestrans`, `includes_url()` and `site_url()`.
pub struct PhpSmilies;

impl Smilies for PhpSmilies {
    fn search(&self) -> Result<Option<Vec<u8>>, TextError> {
        Ok(speedyspec_php::global("wp_smiliessearch").map(|v| text(&v)).filter(|s| !s.is_empty()))
    }

    fn smileys(&self) -> Result<Vec<Vec<u8>>, TextError> {
        Ok(speedyspec_php::global_array_keys("wpsmiliestrans").unwrap_or_default())
    }

    fn image(&self, smiley: &[u8]) -> Result<Option<Vec<u8>>, TextError> {
        Ok(speedyspec_php::global_array_get("wpsmiliestrans", smiley).map(|v| text(&v)))
    }

    fn includes_url(&self, path: &[u8]) -> Result<Vec<u8>, TextError> {
        Ok(text(&call("includes_url", &[bytes_arg(path)])?))
    }

    fn site_url(&self) -> Result<Vec<u8>, TextError> {
        Ok(text(&call("site_url", &[])?))
    }
}

/// `wp_is_internal_link()` (its `wp_internal_hosts` filter fires in PHP).
pub struct PhpLinkContext;

impl LinkContext for PhpLinkContext {
    fn is_internal_link(&self, url: &[u8]) -> Result<bool, TextError> {
        Ok(truthy(&call("wp_is_internal_link", &[bytes_arg(url)])?))
    }
}
