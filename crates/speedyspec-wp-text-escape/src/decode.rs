//! Quote styles, and `wp_specialchars_decode()`.

use std::borrow::Cow;

use memchr::memchr;
use speedyspec_hooks::Value;

/// Which quotes a WordPress escaping function touches, read from its `$quote_style` argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuoteStyle {
    /// `ENT_NOQUOTES`, or any empty value.
    NoQuotes,
    /// `ENT_COMPAT` or `'double'`: double quotes only.
    Double,
    /// `ENT_QUOTES`, and any value WordPress does not recognize.
    Quotes,
    /// `'single'`: single quotes only.
    Single,
    /// `ENT_XML1` (for `_wp_specialchars()` only): both quotes, the XML way.
    Xml,
}

impl QuoteStyle {
    fn from_php(value: &Value, xml: bool) -> QuoteStyle {
        let empty = match value {
            Value::Null => true,
            Value::Bool(b) => !b,
            Value::Int(i) => *i == 0,
            Value::Float(f) => *f == 0.0,
            Value::Str(s) => s.is_empty() || s == "0",
            Value::Bytes(b) => b.is_empty() || b == b"0",
            Value::Array(pairs) => pairs.is_empty(),
            Value::Opaque(_) => false,
        };
        if empty {
            return QuoteStyle::NoQuotes;
        }
        match value {
            Value::Int(16) if xml => QuoteStyle::Xml,
            Value::Int(2) => QuoteStyle::Double,
            Value::Int(3) => QuoteStyle::Quotes,
            Value::Str(s) if s == "double" => QuoteStyle::Double,
            Value::Str(s) if s == "single" => QuoteStyle::Single,
            _ => QuoteStyle::Quotes,
        }
    }

    /// `_wp_specialchars()`'s reading: `ENT_XML1` is [`QuoteStyle::Xml`].
    pub fn for_specialchars(value: &Value) -> QuoteStyle {
        QuoteStyle::from_php(value, true)
    }

    /// `wp_specialchars_decode()`'s reading, which has no XML branch: `ENT_XML1` is [`QuoteStyle::Quotes`].
    pub fn for_decode(value: &Value) -> QuoteStyle {
        QuoteStyle::from_php(value, false)
    }

    fn single(self) -> bool {
        matches!(self, QuoteStyle::Quotes | QuoteStyle::Single | QuoteStyle::Xml)
    }

    fn double(self) -> bool {
        matches!(self, QuoteStyle::Quotes | QuoteStyle::Double | QuoteStyle::Xml)
    }
}

/// `wp_specialchars_decode()`: turns WordPress's own references for `< > &` (and the quotes `style` names) back into
/// characters, in one pass that never rescans its output, so `&amp;lt;` becomes `&lt;`.
pub fn wp_specialchars_decode(text: &[u8], style: QuoteStyle) -> Cow<'_, [u8]> {
    let Some(first) = memchr(b'&', text) else {
        return Cow::Borrowed(text);
    };
    let mut out = Vec::with_capacity(text.len());
    out.extend_from_slice(&text[..first]);
    let mut i = first;
    while i < text.len() {
        if text[i] != b'&' {
            let next = memchr(b'&', &text[i..]).map_or(text.len(), |at| i + at);
            out.extend_from_slice(&text[i..next]);
            i = next;
            continue;
        }
        match decoded(&text[i..], style) {
            Some((byte, len)) => {
                out.push(byte);
                i += len;
            }
            None => {
                out.push(b'&');
                i += 1;
            }
        }
    }
    Cow::Owned(out)
}

/// The character the reference at the start of `s` decodes to, and the reference's length.
fn decoded(s: &[u8], style: QuoteStyle) -> Option<(u8, usize)> {
    for (name, byte, wanted) in [
        (&b"&lt;"[..], b'<', true),
        (b"&gt;", b'>', true),
        (b"&amp;", b'&', true),
        (b"&quot;", b'"', style.double()),
    ] {
        if wanted && s.starts_with(name) {
            return Some((byte, name.len()));
        }
    }
    if s.get(1) != Some(&b'#') {
        return None;
    }
    let hex = matches!(s.get(2), Some(b'x' | b'X'));
    let mut i = if hex { 3 } else { 2 };
    while s.get(i) == Some(&b'0') {
        i += 1;
    }
    let digits: &[(&[u8], u8, bool)] = if hex {
        &[
            (b"27;", b'\'', style.single()),
            (b"22;", b'"', style.double()),
            (b"26;", b'&', true),
        ]
    } else {
        &[
            (b"39;", b'\'', style.single()),
            (b"34;", b'"', style.double()),
            (b"60;", b'<', true),
            (b"62;", b'>', true),
            (b"38;", b'&', true),
        ]
    };
    digits
        .iter()
        .find(|(tail, _, wanted)| *wanted && s[i..].starts_with(tail))
        .map(|(tail, byte, _)| (*byte, i + tail.len()))
}
