//! A start tag's attributes, read as `WP_HTML_Tag_Processor` reads them.

use std::borrow::Cow;

use crate::decode_attribute;

/// One attribute as the Tag Processor reports it: its comparable name (ASCII-lowercased, NUL as U+FFFD) and its
/// decoded value, `None` for an attribute written without one (`get_attribute()` returning `true`). Both borrow the
/// input where reading them changes nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagAttribute<'a> {
    pub name: Cow<'a, [u8]>,
    pub value: Option<Cow<'a, [u8]>>,
}

/// Up to this many attributes, a repeated name is found by comparing with each one kept; past it, with a set.
const LINEAR_DEDUP: usize = 16;

const WHITESPACE: &[u8] = b" \t\x0C\r\n";

fn span_of(html: &[u8], from: usize, set: &[u8]) -> usize {
    html[from..].iter().take_while(|b| set.contains(b)).count()
}

fn span_not_of(html: &[u8], from: usize, set: &[u8]) -> usize {
    html.get(from..)
        .unwrap_or_default()
        .iter()
        .take_while(|b| !set.contains(b))
        .count()
}

/// The attributes of `<wp {attr}>`, as `wp_kses_hair()` reads them through the Tag Processor: first occurrence of
/// each comparable name wins, in source order. `None` when the tag is incomplete (an unclosed quote), where the
/// Tag Processor reports no attributes at all.
pub fn start_tag_attributes(attr: &[u8]) -> Option<Vec<TagAttribute<'_>>> {
    thread_local! {
        static BUFFER: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    BUFFER.with(|buffer| {
        let mut html = buffer.take();
        html.clear();
        html.extend_from_slice(b"<wp ");
        html.extend_from_slice(attr);
        html.push(b'>');
        let found = attributes_of(&html, attr);
        buffer.replace(html);
        found
    })
}

/// [`start_tag_attributes`] over `html`, which is `<wp ` + `attr` + `>`.
fn attributes_of<'a>(html: &[u8], attr: &'a [u8]) -> Option<Vec<TagAttribute<'a>>> {
    let len = html.len();
    let mut at = 3;
    // `html` is `<wp ` + `attr` + `>`: a slice of it inside `attr` is `attr[start - 4..end - 4]`.
    let inner = |start: usize, end: usize| &attr[start - 4..end - 4];
    let mut attributes: Vec<TagAttribute<'_>> = Vec::with_capacity(8);
    let mut seen: Option<std::collections::HashSet<Vec<u8>>> = None;
    loop {
        at += span_of(html, at, b" \t\x0C\r\n/");
        if at >= len {
            return None;
        }
        if html[at] == b'>' {
            break;
        }
        let name_length = if html[at] == b'=' {
            1 + span_not_of(html, at + 1, b"=/> \t\x0C\r\n")
        } else {
            span_not_of(html, at, b"=/> \t\x0C\r\n")
        };
        if name_length == 0 || at + name_length >= len {
            break;
        }
        let attribute_start = at;
        let name = inner(at, at + name_length);
        at += name_length;
        at += span_of(html, at, WHITESPACE);
        if at >= len {
            return None;
        }
        let (value, attribute_end) = if html[at] == b'=' {
            at += 1;
            at += span_of(html, at, WHITESPACE);
            if at >= len {
                return None;
            }
            match html[at] {
                quote @ (b'\'' | b'"') => {
                    let value_start = at + 1;
                    let end_quote_at = memchr::memchr(quote, &html[value_start..])
                        .map_or(len, |q| value_start + q);
                    at = end_quote_at + 1;
                    if end_quote_at + 1 >= len {
                        return None;
                    }
                    (Some(inner(value_start, end_quote_at)), end_quote_at + 1)
                }
                _ => {
                    let value_start = at;
                    let value_length = span_not_of(html, value_start, b"> \t\x0C\r\n");
                    at = value_start + value_length;
                    (Some(inner(value_start, at)), at)
                }
            }
        } else {
            (None, attribute_start + name_length)
        };
        if attribute_end >= len {
            return None;
        }
        let comparable = comparable_name(name);
        let first = match &mut seen {
            Some(set) => set.insert(comparable.to_vec()),
            None if attributes.len() < LINEAR_DEDUP => {
                attributes.iter().all(|a| a.name != comparable)
            }
            None => {
                let mut set: std::collections::HashSet<Vec<u8>> =
                    attributes.iter().map(|a| a.name.to_vec()).collect();
                let first = set.insert(comparable.to_vec());
                seen = Some(set);
                first
            }
        };
        if first {
            attributes.push(TagAttribute {
                name: comparable,
                value: value.map(decoded_value),
            });
        }
    }
    memchr::memchr(b'>', &html[at..])?;
    Some(attributes)
}

/// `strtolower( str_replace( "\x00", "\u{FFFD}", $name ) )`.
fn comparable_name(name: &[u8]) -> Cow<'_, [u8]> {
    if !name.iter().any(|b| *b == 0 || b.is_ascii_uppercase()) {
        return Cow::Borrowed(name);
    }
    let mut out = Vec::with_capacity(name.len());
    for b in name {
        match b {
            0 => out.extend_from_slice("\u{FFFD}".as_bytes()),
            b => out.push(b.to_ascii_lowercase()),
        }
    }
    Cow::Owned(out)
}

/// `get_decoded_attribute_value()`: newlines normalized, NUL as U+FFFD, then decoded as an attribute.
fn decoded_value(raw: &[u8]) -> Cow<'_, [u8]> {
    if !raw.iter().any(|b| matches!(b, b'\r' | 0 | b'&')) {
        return Cow::Borrowed(raw);
    }
    let mut normalized = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            b'\r' => {
                normalized.push(b'\n');
                if raw.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
            }
            0 => normalized.extend_from_slice("\u{FFFD}".as_bytes()),
            b => normalized.push(b),
        }
        i += 1;
    }
    Cow::Owned(decode_attribute(&normalized).into_owned())
}
