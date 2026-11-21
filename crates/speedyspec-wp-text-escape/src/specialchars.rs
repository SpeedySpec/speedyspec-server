//! `_wp_specialchars()` and `wp_check_invalid_utf8()`, once their settings are known.

use std::borrow::Cow;

use speedyspec_wp_kses_contracts::EntityContext;
use speedyspec_wp_kses_lexical::{default_html_entity_names, default_xml_entity_names, normalize_entities};

use crate::charset::{Charset, canonical_charset};
use crate::decode::QuoteStyle;
use crate::htmlspecialchars::{EntFlags, htmlspecialchars};

/// `_wp_specialchars( $text, $quote_style, $charset, $double_encode )` with the charset already chosen.
///
/// @param text
///     The text.
/// @param style
///     The quote style, as [`QuoteStyle::for_specialchars`] reads WordPress's argument.
/// @param charset
///     The charset name (WordPress's `$charset`, or `blog_charset` when that is falsy); it is canonicalized as
///     `_canonical_charset()` does.
/// @param double_encode
///     Whether existing references are escaped again; when false, kses normalizes them first.
/// @return
///     The escaped text; borrowed when it has none of `& < > " '`.
pub fn wp_specialchars<'a>(text: &'a [u8], style: QuoteStyle, charset: &str, double_encode: bool) -> Cow<'a, [u8]> {
    if text.is_empty() || !text.iter().any(|b| matches!(b, b'&' | b'<' | b'>' | b'"' | b'\'')) {
        return Cow::Borrowed(text);
    }
    let charset = Charset::from_php_name(&canonical_charset(charset));
    let normalized = if double_encode {
        Cow::Borrowed(text)
    } else {
        let context = if style == QuoteStyle::Xml { EntityContext::Xml } else { EntityContext::Html };
        normalize_entities(text, context, default_html_entity_names(), default_xml_entity_names())
    };
    let flags = match style {
        QuoteStyle::NoQuotes | QuoteStyle::Single => EntFlags { double_quotes: false, single_quotes: false, xml1: false },
        QuoteStyle::Double => EntFlags { double_quotes: true, single_quotes: false, xml1: false },
        QuoteStyle::Quotes => EntFlags { double_quotes: true, single_quotes: true, xml1: false },
        QuoteStyle::Xml => EntFlags { double_quotes: true, single_quotes: true, xml1: true },
    };
    let escaped = match normalized {
        Cow::Borrowed(b) => htmlspecialchars(b, flags, &charset, double_encode),
        Cow::Owned(o) => Cow::Owned(htmlspecialchars(&o, flags, &charset, double_encode).into_owned()),
    };
    if style == QuoteStyle::Single && memchr::memchr(b'\'', &escaped).is_some() {
        let mut out = Vec::with_capacity(escaped.len() + 16);
        for &b in escaped.iter() {
            if b == b'\'' {
                out.extend_from_slice(b"&#039;");
            } else {
                out.push(b);
            }
        }
        return Cow::Owned(out);
    }
    escaped
}

/// `wp_check_invalid_utf8( $text, $strip )` once the site's charset is known.
///
/// @param text
///     The text.
/// @param is_utf8
///     Whether `blog_charset` is UTF-8 (`is_utf8_charset()`); otherwise the text is returned as it is.
/// @param strip
///     On invalid UTF-8, scrub it to U+FFFD instead of returning `""`.
/// @return
///     The text, `""`, or the scrubbed text.
pub fn check_invalid_utf8_for(text: &[u8], is_utf8: bool, strip: bool) -> Cow<'_, [u8]> {
    if !is_utf8 {
        return Cow::Borrowed(text);
    }
    speedyspec_wp_utf8::check_invalid_utf8(text, strip)
}
