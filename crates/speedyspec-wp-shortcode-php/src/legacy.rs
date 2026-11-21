//! Adapters onto WordPress's own PHP: `_doing_it_wrong()` and `__()`, the kses functions and `wp_html_split()`
//! (until their Rust ports land), and PHP's string conversion.

use speedyspec_hooks::Value;
use speedyspec_wp_shortcode_contracts::{
    DiagnosticMessage, Diagnostics, HtmlSplitter, KsesAttributes, PhpKey, PhpStrings, PhpValue, ShortcodeError, php_bytes,
    php_str_bytes,
};

use crate::handler::thrown;

fn call(function: &str, args: &[Value]) -> Option<Value> {
    match speedyspec_php::call(&Value::Str(function.into()), args) {
        Ok(v) => Some(v),
        Err(err) => {
            eprintln!("speedyspec_wordpress: {function}() failed: {err}");
            None
        }
    }
}

/// Slices `whole` into consecutive pieces of the lengths PHP returned, so they borrow from it; `None` when PHP's
/// pieces are not consecutive slices of it.
fn reslice<'a>(whole: &'a [u8], pieces: &Value) -> Option<Vec<&'a [u8]>> {
    let Value::Array(entries) = pieces else {
        return None;
    };
    let mut at = 0;
    let mut out = Vec::with_capacity(entries.len());
    for (_, piece) in entries {
        let bytes = php_str_bytes(piece)?;
        let slice = whole.get(at..at + bytes.len())?;
        if slice != bytes {
            return None;
        }
        out.push(slice);
        at += bytes.len();
    }
    Some(out)
}

/// WordPress's notices through its own functions, so translations and `doing_it_wrong_run` behave as before.
pub struct PhpDiagnostics;

impl Diagnostics for PhpDiagnostics {
    fn doing_it_wrong(&self, function: &str, message: DiagnosticMessage, version: &str) {
        let Some(template) = call("__", &[Value::Str(message.template.into())]) else {
            return;
        };
        let mut args = vec![template];
        args.extend(message.args.into_iter().map(php_bytes));
        let Some(text) = call("sprintf", &args) else {
            return;
        };
        call("_doing_it_wrong", &[Value::Str(function.into()), text, Value::Str(version.into())]);
    }

    /// Raises the very deprecation WordPress raises, by making the same call.
    fn deprecated_null_subject(&self, function: &str) {
        let _ = speedyspec_php::call_catching(&Value::Str(function.into()), &[Value::Str("/x/".into()), Value::Null]);
    }

    /// Raises PHP's own warning, by reading the missing key.
    fn undefined_array_key(&self, key: &PhpKey) {
        let read = Value::Str(r"SpeedySpec\WpShortcode\Support\read_missing_key".into());
        let key = match key {
            PhpKey::Int(i) => Value::Int(*i),
            PhpKey::Str(s) => Value::Str(s.clone()),
        };
        let _ = speedyspec_php::call_catching(&read, &[key]);
    }
}

/// `wp_html_split()` in WordPress's PHP.
pub struct PhpHtmlSplitter;

impl HtmlSplitter for PhpHtmlSplitter {
    fn split<'a>(&self, content: &'a [u8]) -> Vec<&'a [u8]> {
        call("wp_html_split", &[php_bytes(content.to_vec())])
            .and_then(|parts| reslice(content, &parts))
            .unwrap_or_else(|| vec![content])
    }
}

/// `wp_kses_attr_parse()` and `wp_kses_one_attr()` in WordPress's PHP.
pub struct PhpKsesAttributes;

impl KsesAttributes for PhpKsesAttributes {
    fn attr_parse<'a>(&self, element: &'a [u8]) -> Option<Vec<&'a [u8]>> {
        let parts = call("wp_kses_attr_parse", &[php_bytes(element.to_vec())])?;
        reslice(element, &parts)
    }

    fn one_attr(&self, attr: &[u8], element: &[u8]) -> Vec<u8> {
        call("wp_kses_one_attr", &[php_bytes(attr.to_vec()), php_bytes(element.to_vec())])
            .and_then(|v| php_str_bytes(&v).map(<[u8]>::to_vec))
            .unwrap_or_default()
    }
}

/// PHP's `(string)` conversion: scalars directly, anything else through `strval()`, so arrays warn and objects
/// without `__toString` throw exactly as concatenation does.
pub struct PhpStringConversion;

impl PhpStrings for PhpStringConversion {
    fn to_php_string(&self, value: &PhpValue) -> Result<Vec<u8>, ShortcodeError> {
        if let Some(bytes) = php_str_bytes(value) {
            return Ok(bytes.to_vec());
        }
        match value {
            Value::Null | Value::Bool(false) => return Ok(Vec::new()),
            Value::Bool(true) => return Ok(b"1".to_vec()),
            Value::Int(i) => return Ok(i.to_string().into_bytes()),
            _ => {}
        }
        let converted = speedyspec_php::call_catching(&Value::Str("strval".into()), std::slice::from_ref(value))
            .map_err(thrown)?;
        php_str_bytes(&converted)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| ShortcodeError::Conversion("strval() returned a non-string".into()))
    }
}
