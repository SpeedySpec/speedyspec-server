//! The responsive breakpoints (`settings.viewport`), new in WordPress 7.0.

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::schema;

use crate::array_path;
use crate::codec::string_value;

/// PHP's `trim()` with its default characters.
fn php_trim(s: &[u8]) -> &[u8] {
    let is_ws = |c: &u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\0' | b'\x0b');
    let start = s.iter().position(|c| !is_ws(c)).unwrap_or(s.len());
    let end = s.iter().rposition(|c| !is_ws(c)).map_or(start, |e| e + 1);
    &s[start..end]
}

/// `is_valid_viewport_breakpoint_size`: `/^(?:\d+|\d*\.\d+)(?:px|em|rem)$/` on the trimmed string.
fn valid_size(value: &[u8]) -> bool {
    let unit_len = [&b"rem"[..], b"px", b"em"]
        .iter()
        .find(|u| value.ends_with(u))
        .map(|u| u.len());
    let Some(unit_len) = unit_len else {
        return false;
    };
    let number = &value[..value.len() - unit_len];
    let digits = |s: &[u8]| s.iter().all(u8::is_ascii_digit);
    match number.iter().position(|c| *c == b'.') {
        None => !number.is_empty() && digits(number),
        Some(dot) => digits(&number[..dot]) && number.len() > dot + 1 && digits(&number[dot + 1..]),
    }
}

/// `get_viewport_breakpoint_value_in_pixels`: the size in pixels, `em` and `rem` at 16px.
fn in_pixels(value: &Value) -> Option<f64> {
    let bytes = match value {
        Value::Str(s) => s.as_bytes(),
        Value::Bytes(b) => b,
        _ => return None,
    };
    let trimmed = php_trim(bytes);
    if trimmed.is_empty() || !valid_size(trimmed) {
        return None;
    }
    let (number, px) = if trimmed.ends_with(b"rem") {
        (&trimmed[..trimmed.len() - 3], false)
    } else {
        (&trimmed[..trimmed.len() - 2], trimmed.ends_with(b"px"))
    };
    let number: f64 = std::str::from_utf8(number).ok()?.parse().unwrap_or(0.0);
    Some(if px { number } else { number * 16.0 })
}

fn defaults() -> Value {
    schema::constant("DEFAULT_VIEWPORT_BREAKPOINTS").unwrap_or(Value::Array(Vec::new()))
}

/// `sanitize_viewport_settings`: valid breakpoints, trimmed; the tablet breakpoint only when wider than mobile; the
/// defaults when none is valid.
pub fn sanitize_viewport_settings(viewport: Option<&Value>) -> Value {
    let Some(viewport @ Value::Array(_)) = viewport else {
        return defaults();
    };
    let mut found: Vec<(&str, Vec<u8>, f64)> = Vec::new();
    for name in ["mobile", "tablet"] {
        let value = array_path::child(viewport, &Key::Str(name.into()))
            .cloned()
            .unwrap_or(Value::Null);
        if let Some(px) = in_pixels(&value) {
            let bytes = match &value {
                Value::Str(s) => s.as_bytes().to_vec(),
                Value::Bytes(b) => b.clone(),
                _ => unreachable!(),
            };
            found.push((name, php_trim(&bytes).to_vec(), px));
        }
    }
    let entry = |name: &str, value: &[u8]| (Key::Str(name.into()), string_value(value));
    match found.as_slice() {
        [] => defaults(),
        [(name, value, _)] => Value::Array(vec![entry(name, value)]),
        [(_, mobile, mobile_px), (_, tablet, tablet_px), ..] => {
            let mut out = vec![entry("mobile", mobile)];
            if mobile_px < tablet_px {
                out.push(entry("tablet", tablet));
            }
            Value::Array(out)
        }
    }
}

fn text(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::Str(s)) => Some(s.clone()),
        Some(Value::Bytes(b)) => Some(String::from_utf8_lossy(b).into_owned()),
        _ => None,
    }
}

/// `WP_Theme_JSON::get_viewport_media_queries( $viewport_settings, $options )`: the breakpoint states (`@mobile`,
/// `@tablet`, and `@desktop` when asked for) and their media queries.
pub fn media_queries(viewport: Option<&Value>, include_desktop: bool) -> Vec<(String, String)> {
    let breakpoints = sanitize_viewport_settings(viewport);
    let mobile = text(array_path::child(&breakpoints, &Key::Str("mobile".into())));
    let tablet = text(array_path::child(&breakpoints, &Key::Str("tablet".into())));
    let mut out = Vec::new();
    if let Some(m) = &mobile {
        out.push(("@mobile".to_owned(), format!("@media (width <= {m})")));
    }
    if let Some(t) = &tablet {
        let query = match &mobile {
            Some(m) => format!("@media ({m} < width <= {t})"),
            None => format!("@media (width <= {t})"),
        };
        out.push(("@tablet".to_owned(), query));
    }
    if include_desktop {
        let desktop = tablet.or(mobile).unwrap_or_default();
        out.push(("@desktop".to_owned(), format!("@media (width > {desktop})")));
    }
    out
}
