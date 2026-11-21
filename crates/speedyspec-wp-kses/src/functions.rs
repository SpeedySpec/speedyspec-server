//! The functions around `wp_kses()`: `wp_kses_one_attr()`, `wp_kses_post()`, `wp_kses_data()`, the `wp_filter_*`
//! family and `_wp_kses_allow_note_mention_span()`.

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_kses_contracts::{AllowedSpec, Context, KsesError};
use speedyspec_wp_kses_lexical::scan::{equals, is_space, leading_space, trailing_space};
use speedyspec_wp_kses_lexical::{bad_protocol, no_null};

use crate::KsesApi;
use crate::api::php_string;

/// PHP's `addslashes()`.
pub fn addslashes(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + 8);
    for b in s {
        match b {
            b'\'' | b'"' | b'\\' => {
                out.push(b'\\');
                out.push(*b);
            }
            0 => out.extend_from_slice(b"\\0"),
            b => out.push(*b),
        }
    }
    out
}

/// PHP's `stripslashes()`.
pub fn php_stripslashes(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'\\' {
            match s.get(i + 1) {
                Some(b'0') => out.push(0),
                Some(b) => out.push(*b),
                None => {}
            }
            i += 2;
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

/// `_wp_kses_allow_note_mention_span()`: lets `span[class]` through comment kses, for note mention chips.
pub fn allow_note_mention_span(allowed: &Value, context: &Value) -> Value {
    let mut allowed = match allowed {
        Value::Array(_) => allowed.clone(),
        _ => Value::Array(Vec::new()),
    };
    if !matches!(context, Value::Str(s) if s == "pre_comment_content") {
        return allowed;
    }
    let Value::Array(items) = &mut allowed else {
        unreachable!()
    };
    let span = items
        .iter_mut()
        .find(|(k, _)| matches!(k, Key::Str(s) if s == "span"));
    let span = match span {
        Some((_, v)) => {
            if !matches!(v, Value::Array(_)) {
                *v = Value::Array(Vec::new());
            }
            v
        }
        None => {
            items.push((Key::Str("span".into()), Value::Array(Vec::new())));
            &mut items.last_mut().expect("just pushed").1
        }
    };
    let Value::Array(attrs) = span else {
        unreachable!()
    };
    match attrs
        .iter_mut()
        .find(|(k, _)| matches!(k, Key::Str(s) if s == "class"))
    {
        Some((_, v)) => *v = Value::Bool(true),
        None => attrs.push((Key::Str("class".into()), Value::Bool(true))),
    }
    allowed
}

impl KsesApi {
    /// `wp_kses_version()`.
    pub fn version() -> &'static str {
        "0.2.2"
    }

    /// The context `current_filter()` names (empty outside any filter).
    fn current_context(&self) -> Context {
        let current = self
            .d
            .hooks
            .current_filter()
            .and_then(|v| php_string(&v.read()))
            .unwrap_or_default();
        Context::from_bytes(&current)
    }

    /// `wp_kses_post()`.
    pub fn post(&self, data: &[u8]) -> Result<Vec<u8>, KsesError> {
        self.kses(data, &AllowedSpec::Context(Context::Post), None)
    }

    /// `wp_kses_data()`: kses with the allowlist named by the current filter.
    pub fn data(&self, data: &[u8]) -> Result<Vec<u8>, KsesError> {
        self.kses(data, &AllowedSpec::Context(self.current_context()), None)
    }

    /// `wp_filter_kses()`: for slashed data, with the allowlist named by the current filter.
    pub fn filter_kses(&self, data: &[u8]) -> Result<Vec<u8>, KsesError> {
        let ctx = self.current_context();
        Ok(addslashes(&self.kses(
            &php_stripslashes(data),
            &AllowedSpec::Context(ctx),
            None,
        )?))
    }

    /// `wp_filter_post_kses()`.
    pub fn filter_post_kses(&self, data: &[u8]) -> Result<Vec<u8>, KsesError> {
        Ok(addslashes(&self.kses(
            &php_stripslashes(data),
            &AllowedSpec::Context(Context::Post),
            None,
        )?))
    }

    /// `wp_filter_nohtml_kses()`.
    pub fn filter_nohtml_kses(&self, data: &[u8]) -> Result<Vec<u8>, KsesError> {
        Ok(addslashes(&self.kses(
            &php_stripslashes(data),
            &AllowedSpec::Context(Context::Strip),
            None,
        )?))
    }

    /// `wp_kses_one_attr()`: one `name=value` attribute, sanitized as kses would sanitize it on `element`, its
    /// surrounding whitespace kept.
    pub fn one_attr(&self, attr: &[u8], element: &[u8]) -> Result<Vec<u8>, KsesError> {
        let uris = self.uri_attributes()?;
        let allowed = self.allowed_html(&Context::Post)?;
        let protocols = self.allowed_protocols()?;
        let attr = no_null(attr, true);
        let lead = leading_space(&attr);
        let trail = trailing_space(&attr);
        let (lead_text, trail_text) = (&attr[..lead], &attr[attr.len() - trail..]);
        let middle: &[u8] = if lead + trail >= attr.len() {
            b""
        } else {
            &attr[lead..attr.len() - trail]
        };
        let split = (0..middle.len())
            .find(|&p| (is_space(middle[p]) || middle[p] == b'=') && equals(&middle[p..]) > 0)
            .map(|p| (p, p + equals(&middle[p..])));
        let (name, attr_text, value, vless) = match split {
            Some((name_end, value_start)) => {
                let name = &middle[..name_end];
                let mut value = &middle[value_start..];
                let quote = match value.first() {
                    Some(q @ (b'"' | b'\'')) => {
                        if !value.ends_with(&[*q]) {
                            return Ok(Vec::new());
                        }
                        value = if value.len() >= 2 {
                            &value[1..value.len() - 1]
                        } else {
                            b""
                        };
                        *q
                    }
                    _ => b'"',
                };
                let mut value = self.d.escaper.esc_attr(value)?;
                if uris.0.iter().any(|u| **u == *name.to_ascii_lowercase()) {
                    value = bad_protocol(&value, &protocols).into_owned();
                }
                let text = [name, b"=", &[quote], &value, &[quote]].concat();
                (name.to_vec(), text, value, false)
            }
            None => (middle.to_vec(), middle.to_vec(), Vec::new(), true),
        };
        let checked = self.attr_check(&name, &value, &attr_text, vless, element, &allowed)?;
        let attr = checked.map(|c| c.whole).unwrap_or_default();
        Ok([lead_text, &attr, trail_text].concat())
    }
}
