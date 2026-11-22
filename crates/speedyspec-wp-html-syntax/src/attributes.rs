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

/// A byte set as a lookup table, for the spans the lexer measures.
const fn byte_set(members: &[u8]) -> [bool; 256] {
    let mut table = [false; 256];
    let mut i = 0;
    while i < members.len() {
        table[members[i] as usize] = true;
        i += 1;
    }
    table
}

/// Whitespace and `/`, skipped before each attribute.
const BEFORE_ATTRIBUTE: [bool; 256] = byte_set(b" \t\x0C\r\n/");
const WHITESPACE: [bool; 256] = byte_set(b" \t\x0C\r\n");
/// What ends an attribute name.
const NAME_END: [bool; 256] = byte_set(b"=/> \t\x0C\r\n");
/// What ends an unquoted value.
const UNQUOTED_END: [bool; 256] = byte_set(b"> \t\x0C\r\n");

/// How many bytes of `s` from `from` are in `set` (none past the end).
fn span_of(s: &[u8], from: usize, set: &[bool; 256]) -> usize {
    s.get(from..)
        .unwrap_or_default()
        .iter()
        .take_while(|b| set[usize::from(**b)])
        .count()
}

/// How many bytes of `s` from `from` are not in `set` (none past the end).
fn span_not_of(s: &[u8], from: usize, set: &[bool; 256]) -> usize {
    s.get(from..)
        .unwrap_or_default()
        .iter()
        .take_while(|b| !set[usize::from(**b)])
        .count()
}

/// The attributes of `<wp {attr}>`, as `wp_kses_hair()` reads them through the Tag Processor: first occurrence of
/// each comparable name wins, in source order. `None` when the tag is incomplete (an unclosed quote), where the
/// Tag Processor reports no attributes at all.
pub fn start_tag_attributes(attr: &[u8]) -> Option<Vec<TagAttribute<'_>>> {
    let mut attributes = Vec::with_capacity(8);
    start_tag_attributes_into(
        attr,
        &mut attributes,
        |a: &TagAttribute<'_>| &a.name,
        |name, value| TagAttribute { name, value },
    )
    .then_some(attributes)
}

/// [`start_tag_attributes`] appending to `out`, so a caller can keep its own form of the attributes in a buffer it
/// reuses.
///
/// @param make
///     Makes the kept form of one attribute from its comparable name and decoded value; called for the first
///     occurrence of each name only.
/// @param name_of
///     Reads back the comparable name of what `make` made, to find repeated names.
/// @return
///     False when the tag is incomplete; `out` is then left as it was.
pub fn start_tag_attributes_into<'a, T>(
    attr: &'a [u8],
    out: &mut Vec<T>,
    name_of: impl Fn(&T) -> &[u8],
    mut make: impl FnMut(Cow<'a, [u8]>, Option<Cow<'a, [u8]>>) -> T,
) -> bool {
    let first_index = out.len();
    let mut seen: Option<std::collections::HashSet<Vec<u8>>> = None;
    let complete = lex(attr, |name, value| {
        let comparable = comparable_name(name);
        let kept = &out[first_index..];
        let first = match &mut seen {
            Some(set) => set.insert(comparable.to_vec()),
            None if kept.len() < LINEAR_DEDUP => kept.iter().all(|a| name_of(a) != &*comparable),
            None => {
                let mut set: std::collections::HashSet<Vec<u8>> =
                    kept.iter().map(|a| name_of(a).to_vec()).collect();
                let first = set.insert(comparable.to_vec());
                seen = Some(set);
                first
            }
        };
        if first {
            out.push(make(comparable, value.map(decoded_value)));
        }
    });
    if !complete {
        out.truncate(first_index);
    }
    complete
}

/// The Tag Processor's attribute lexing over `<wp {attr}>`, read in place without building that tag: each
/// attribute's raw name and raw value (`None` when written without one) in source order, repeats included.
///
/// Positions count from the start of `attr`; the tag's closing `>` sits just past its end, at `attr.len()`.
///
/// @return
///     False when the tag is incomplete (an unclosed quote).
fn lex<'a>(attr: &'a [u8], mut each: impl FnMut(&'a [u8], Option<&'a [u8]>)) -> bool {
    let n = attr.len();
    // The tag's length from here on: the attributes and the closing `>`.
    let len = n + 1;
    let byte = |at: usize| if at < n { attr[at] } else { b'>' };
    let mut at = 0;
    loop {
        at += span_of(attr, at, &BEFORE_ATTRIBUTE);
        if at >= len {
            return false;
        }
        if byte(at) == b'>' {
            break;
        }
        let name_length = if attr[at] == b'=' {
            1 + span_not_of(attr, at + 1, &NAME_END)
        } else {
            span_not_of(attr, at, &NAME_END)
        };
        if name_length == 0 || at + name_length >= len {
            break;
        }
        let attribute_start = at;
        let name = &attr[at..at + name_length];
        at += name_length;
        at += span_of(attr, at, &WHITESPACE);
        if at >= len {
            return false;
        }
        let (value, attribute_end) = if byte(at) == b'=' {
            at += 1;
            at += span_of(attr, at, &WHITESPACE);
            if at >= len {
                return false;
            }
            match byte(at) {
                quote @ (b'\'' | b'"') => {
                    let value_start = at + 1;
                    // The closing `>` is no quote, so the search stays inside `attr`.
                    let end_quote_at = memchr::memchr(quote, &attr[value_start..])
                        .map_or(len, |q| value_start + q);
                    at = end_quote_at + 1;
                    if end_quote_at + 1 >= len {
                        return false;
                    }
                    (Some(&attr[value_start..end_quote_at]), end_quote_at + 1)
                }
                _ => {
                    let value_start = at;
                    let value_length = span_not_of(attr, value_start, &UNQUOTED_END);
                    at = value_start + value_length;
                    (Some(&attr[value_start..at]), at)
                }
            }
        } else {
            (None, attribute_start + name_length)
        };
        if attribute_end >= len {
            return false;
        }
        each(name, value);
    }
    true
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
    if memchr::memchr3(b'\r', 0, b'&', raw).is_none() {
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
