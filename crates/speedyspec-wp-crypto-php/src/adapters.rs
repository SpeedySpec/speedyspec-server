//! The ports WordPress's rules need, over the Hook API and PHP.

use std::rc::Rc;

use base64::Engine as _;
use speedyspec_php::native::NativeError;
use speedyspec_wp_crypto_contracts::{Clock, FilteredKeys, Translator, TrustedKeysFilter};
use speedyspec_wp_hooks::{Count, HookApi, HookError, HookName, HookValue};
use speedyspec_wp_text_contracts::{
    Diagnostics, ErrorLevel, SiteSettings, TextError, WordCountType,
};

/// The `wp_trusted_keys` filter over the Hook API.
///
/// With no callback the run is only counted (`did_filter()` stays WordPress's) and PHP is not entered. The filtered
/// value is kept as the filter returned it, for WordPress's error data, and its elements are read as strings the way
/// `base64_decode()` reads them. A callback's exception, or `base64_decode()`'s `TypeError` on a key that can't be a
/// string, is the call's failure.
pub struct HookTrustedKeysFilter {
    hooks: Rc<HookApi>,
    name: HookName,
}

impl HookTrustedKeysFilter {
    /// The filter over `hooks`.
    pub fn new(hooks: Rc<HookApi>) -> HookTrustedKeysFilter {
        HookTrustedKeysFilter {
            hooks,
            name: HookName::from("wp_trusted_keys"),
        }
    }

    /// Filters `defaults`, failing as the PHP call would.
    pub fn try_apply(
        &self,
        defaults: Vec<Vec<u8>>,
    ) -> Result<FilteredKeys<HookValue>, NativeError> {
        let value = php_list(&defaults);
        let raw = if self
            .hooks
            .skip_if_empty(&self.name, Count::Filters)
            .map_err(native_error)?
        {
            value
        } else {
            self.hooks
                .apply_filters(&self.name, value, Vec::new())
                .map_err(native_error)?
        };
        let keys = elements(&raw.read())
            .iter()
            .map(key_string)
            .collect::<Result<_, _>>()?;
        Ok(FilteredKeys { keys, raw })
    }
}

impl TrustedKeysFilter for HookTrustedKeysFilter {
    type Raw = Result<HookValue, NativeError>;

    fn apply(&self, defaults: Vec<Vec<u8>>) -> FilteredKeys<Self::Raw> {
        match self.try_apply(defaults) {
            Ok(filtered) => FilteredKeys {
                keys: filtered.keys,
                raw: Ok(filtered.raw),
            },
            Err(err) => FilteredKeys {
                keys: Vec::new(),
                raw: Err(err),
            },
        }
    }
}

/// A list of strings as a PHP list.
pub(crate) fn php_list(items: &[Vec<u8>]) -> HookValue {
    HookValue::Array(
        items
            .iter()
            .enumerate()
            .map(|(i, item)| (speedyspec_hooks::Key::Int(i as i64), php_string(item)))
            .collect(),
    )
}

/// Bytes as a PHP string value.
pub(crate) fn php_string(bytes: &[u8]) -> HookValue {
    match std::str::from_utf8(bytes) {
        Ok(s) => HookValue::Str(s.to_owned()),
        Err(_) => HookValue::Bytes(bytes.to_vec()),
    }
}

/// `(array) $value`'s elements, in order.
fn elements(value: &HookValue) -> Vec<HookValue> {
    match value {
        HookValue::Null => Vec::new(),
        HookValue::Array(items) => items.iter().map(|(_, v)| v.clone()).collect(),
        other => vec![other.clone()],
    }
}

/// A key as the string `base64_decode()` sees: strings as they are, anything else through PHP's own `base64_decode()`
/// (and back), so its coercions and errors are PHP's.
fn key_string(value: &HookValue) -> Result<Vec<u8>, NativeError> {
    match value.read().as_ref() {
        HookValue::Str(s) => return Ok(s.as_bytes().to_vec()),
        HookValue::Bytes(b) => return Ok(b.clone()),
        _ => {}
    }
    let decoded = speedyspec_php::call_catching(
        &HookValue::Str("base64_decode".into()),
        std::slice::from_ref(value),
    )
    .map_err(|failure| match failure.exception {
        Some(exception) => NativeError::Rethrow(exception),
        None => NativeError::Silent,
    })?;
    let bytes = match decoded.read().as_ref() {
        HookValue::Str(s) => s.as_bytes().to_vec(),
        HookValue::Bytes(b) => b.clone(),
        _ => Vec::new(),
    };
    Ok(base64::engine::general_purpose::STANDARD
        .encode(bytes)
        .into_bytes())
}

/// A Hook API failure as the native function's error: the PHP exception rethrown when there is one.
pub(crate) fn native_error(err: HookError) -> NativeError {
    match &err.payload {
        Some(payload) => match payload.downcast_ref::<HookValue>() {
            Some(exception) => NativeError::Rethrow(exception.clone()),
            None => NativeError::Silent,
        },
        None => NativeError::throw("Error", err.message),
    }
}

/// WordPress's `__()` when it is loaded; the text itself otherwise (the runtime runs without WordPress).
pub struct PhpTranslator;

impl Translator for PhpTranslator {
    fn translate(&self, text: &str) -> String {
        if !function_exists("__") {
            return text.to_string();
        }
        match speedyspec_php::call(&HookValue::Str("__".into()), &[HookValue::Str(text.into())]) {
            Ok(HookValue::Str(s)) => s,
            Ok(HookValue::Bytes(b)) => String::from_utf8_lossy(&b).into_owned(),
            _ => text.to_string(),
        }
    }
}

fn function_exists(name: &str) -> bool {
    matches!(
        speedyspec_php::call(
            &HookValue::Str("function_exists".into()),
            &[HookValue::Str(name.into())]
        ),
        Ok(HookValue::Bool(true))
    )
}

/// `time()`.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
    }
}

/// The site settings `esc_html()` reads: `blog_charset` through `get_option()` when WordPress is loaded, UTF-8
/// otherwise. The other settings are never read by `esc_html()`.
pub struct PhpSiteSettings;

impl SiteSettings for PhpSiteSettings {
    fn blog_charset(&self) -> Result<String, TextError> {
        if !function_exists("get_option") {
            return Ok("UTF-8".to_string());
        }
        match speedyspec_php::call(
            &HookValue::Str("get_option".into()),
            &[HookValue::Str("blog_charset".into())],
        ) {
            Ok(HookValue::Str(s)) => Ok(s),
            Ok(_) => Ok(String::new()),
            Err(err) => Err(TextError::message(err.to_string())),
        }
    }

    fn use_smilies(&self) -> Result<bool, TextError> {
        Ok(false)
    }

    fn use_balance_tags(&self) -> Result<bool, TextError> {
        Ok(false)
    }

    fn word_count_type(&self) -> Result<WordCountType, TextError> {
        Ok(WordCountType::Words)
    }
}

/// `esc_html()` reports nothing, so its diagnostics port has nothing to do.
pub struct QuietDiagnostics;

impl Diagnostics for QuietDiagnostics {
    fn deprecated_function(&self, _: &str, _: &str, _: &str) -> Result<(), TextError> {
        Ok(())
    }

    fn deprecated_argument(&self, _: &str, _: &str, _: &str) -> Result<(), TextError> {
        Ok(())
    }

    fn trigger_error(&self, _: &str, _: &str, _: ErrorLevel) -> Result<(), TextError> {
        Ok(())
    }
}
