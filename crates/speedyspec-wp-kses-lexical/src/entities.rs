//! `wp_kses_normalize_entities()` and `wp_kses_decode_entities()`.

use std::borrow::Cow;
use std::sync::OnceLock;

use speedyspec_wp_kses_contracts::{EntityContext, EntityNames};

use crate::entity_names::{HTML_ENTITY_NAMES, XML_ENTITY_NAMES};

/// WordPress's default `$allowedentitynames`.
pub fn default_html_entity_names() -> &'static EntityNames {
    static NAMES: OnceLock<EntityNames> = OnceLock::new();
    NAMES.get_or_init(|| EntityNames(HTML_ENTITY_NAMES.iter().map(|n| (*n).into()).collect()))
}

/// WordPress's default `$allowedxmlentitynames`.
pub fn default_xml_entity_names() -> &'static EntityNames {
    static NAMES: OnceLock<EntityNames> = OnceLock::new();
    NAMES.get_or_init(|| EntityNames(XML_ENTITY_NAMES.iter().map(|n| (*n).into()).collect()))
}

/// WordPress's default `$allowedentitynames` as the list it is, in WordPress's order.
pub fn default_html_entity_name_list() -> &'static [&'static [u8]] {
    HTML_ENTITY_NAMES
}

/// `valid_unicode()`: a code point XML allows.
pub fn valid_unicode(i: u64) -> bool {
    matches!(i, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

fn run(s: &[u8], from: usize, f: impl Fn(u8) -> bool) -> usize {
    s.get(from..)
        .unwrap_or_default()
        .iter()
        .take_while(|b| f(**b))
        .count()
}

fn value(digits: &[u8], radix: u32) -> u64 {
    digits.iter().fold(0u64, |v, d| {
        v * radix as u64 + (*d as char).to_digit(radix).unwrap_or(0) as u64
    })
}

/// The rewrite of one `&` at `rest[0]` and how many bytes of `rest` it consumes, as the three
/// `preg_replace_callback()` passes after `str_replace( '&', '&amp;' )` would leave it.
fn rewrite(
    rest: &[u8],
    ctx: EntityContext,
    html: &EntityNames,
    xml: &EntityNames,
    out: &mut Vec<u8>,
) -> usize {
    if rest.get(1) == Some(&b'#') {
        let hex = matches!(rest.get(2), Some(b'x' | b'X'));
        let digits_at = if hex { 3 } else { 2 };
        let zeros = run(rest, digits_at, |b| b == b'0');
        let first = rest.get(digits_at + zeros).copied();
        let (digit_ok, max): (fn(u8) -> bool, usize) = if hex {
            (|b| b.is_ascii_hexdigit(), 6)
        } else {
            (|b| b.is_ascii_digit(), 7)
        };
        let starts = first.is_some_and(|b| digit_ok(b) && b != b'0');
        let count = run(rest, digits_at + zeros, digit_ok);
        let end = digits_at + zeros + count;
        if starts && count <= max && rest.get(end) == Some(&b';') {
            let all = &rest[digits_at..end];
            let significant = &rest[digits_at + zeros..end];
            let ok = valid_unicode(value(significant, if hex { 16 } else { 10 }));
            match (hex, ok) {
                (true, true) => out.extend_from_slice(&[b"&#x", significant, b";"].concat()),
                (true, false) => out.extend_from_slice(&[b"&amp;#x", all, b";"].concat()),
                (false, true) => {
                    out.extend_from_slice(b"&#");
                    out.extend(std::iter::repeat_n(
                        b'0',
                        3usize.saturating_sub(significant.len()),
                    ));
                    out.extend_from_slice(significant);
                    out.push(b';');
                }
                (false, false) => out.extend_from_slice(&[b"&amp;#", all, b";"].concat()),
            }
            return end + 1;
        }
        out.extend_from_slice(b"&amp;");
        return 1;
    }
    let letters = run(rest, 1, |b| b.is_ascii_alphabetic());
    let digits = run(rest, 1 + letters, |b| b.is_ascii_digit());
    let end = 1 + letters + digits;
    if (2..=8).contains(&letters) && digits <= 2 && rest.get(end) == Some(&b';') {
        let name = &rest[1..end];
        if ctx == EntityContext::Xml && xml.0.contains(name) {
            out.extend_from_slice(&rest[..=end]);
        } else if html.0.contains(name) {
            match ctx {
                EntityContext::Html => out.extend_from_slice(&rest[..=end]),
                EntityContext::Xml => out.extend_from_slice(&html_entity_decode(&rest[..=end])),
            }
        } else {
            out.extend_from_slice(b"&amp;");
            out.extend_from_slice(&rest[1..=end]);
        }
        return end + 1;
    }
    out.extend_from_slice(b"&amp;");
    1
}

/// `html_entity_decode( "&name;", ENT_HTML5 )` for one complete reference; unchanged when HTML5 has no such name.
pub fn html_entity_decode(reference: &[u8]) -> Vec<u8> {
    match speedyspec_wp_html_syntax::named_reference(reference, 1) {
        Some((replacement, len)) if len + 1 == reference.len() => replacement.to_vec(),
        _ => reference.to_vec(),
    }
}

/// `wp_kses_normalize_entities()`: every `&` that does not start a reference kses keeps becomes `&amp;`, in one
/// pass; borrows when there is no `&`.
pub fn normalize_entities<'a>(
    content: &'a [u8],
    ctx: EntityContext,
    html_names: &EntityNames,
    xml_names: &EntityNames,
) -> Cow<'a, [u8]> {
    let Some(first) = memchr::memchr(b'&', content) else {
        return Cow::Borrowed(content);
    };
    let mut out = Vec::with_capacity(content.len() + content.len() / 8 + 8);
    out.extend_from_slice(&content[..first]);
    let mut at = first;
    loop {
        at += rewrite(&content[at..], ctx, html_names, xml_names, &mut out);
        match memchr::memchr(b'&', &content[at..]) {
            Some(next) => {
                out.extend_from_slice(&content[at..at + next]);
                at += next;
            }
            None => {
                out.extend_from_slice(&content[at..]);
                return Cow::Owned(out);
            }
        }
    }
}

/// One `preg_replace_callback()` pass of `wp_kses_decode_entities()`: `&#` + optional `x`, digits, `;` becomes
/// `chr()` of the number. A number beyond PHP's integer range is left as it is (PHP throws a `TypeError` there).
fn decode_pass(content: &[u8], hex: bool) -> Cow<'_, [u8]> {
    let mut out: Option<Vec<u8>> = None;
    let mut at = 0;
    let mut copied = 0;
    while let Some(found) = memchr::memmem::find(&content[at..], b"&#") {
        let start = at + found;
        let mut digits_at = start + 2;
        if hex {
            if !matches!(content.get(digits_at), Some(b'x' | b'X')) {
                at = start + 2;
                continue;
            }
            digits_at += 1;
        }
        let radix = if hex { 16 } else { 10 };
        let count = run(content, digits_at, |b| (b as char).is_digit(radix));
        let end = digits_at + count;
        if count == 0 || content.get(end) != Some(&b';') {
            at = start + 2;
            continue;
        }
        let digits = &content[digits_at..end];
        let parsed = u64::from_str_radix(std::str::from_utf8(digits).unwrap_or("x"), radix)
            .ok()
            .filter(|v| *v <= i64::MAX as u64);
        if let Some(v) = parsed {
            let o = out.get_or_insert_with(|| Vec::with_capacity(content.len()));
            o.extend_from_slice(&content[copied..start]);
            o.push((v % 256) as u8);
            copied = end + 1;
        }
        at = end + 1;
    }
    match out {
        None => Cow::Borrowed(content),
        Some(mut o) => {
            o.extend_from_slice(&content[copied..]);
            Cow::Owned(o)
        }
    }
}

/// `wp_kses_decode_entities()`: decimal references, then hexadecimal ones, each to one byte as `chr()` does.
pub fn decode_entities(content: &[u8]) -> Cow<'_, [u8]> {
    match decode_pass(content, false) {
        Cow::Borrowed(b) => decode_pass(b, true),
        Cow::Owned(o) => Cow::Owned(decode_pass(&o, true).into_owned()),
    }
}
