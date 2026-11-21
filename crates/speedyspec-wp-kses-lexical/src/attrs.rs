//! `wp_kses_hair()`, `wp_kses_attr_parse()` and `wp_kses_hair_parse()`.

use std::borrow::Cow;

use speedyspec_wp_html_syntax::start_tag_attributes;
use speedyspec_wp_kses_contracts::{Attr, Protocols, UriAttributes};

use crate::bad_protocol;
use crate::scan::{attr_name, is_space, leading_space, trailing_space};

/// `wp_kses_hair()` as WordPress 7.0+ writes it: the attributes as the HTML API's Tag Processor reads them (names
/// lowercased, first occurrence wins, values decoded), URL values cleaned with [`bad_protocol`], and every value
/// re-encoded inside double quotes.
pub fn hair<'a>(attr: &'a [u8], protocols: &Protocols, uris: &UriAttributes) -> Vec<Attr<'a>> {
    hair_attrs(attr, protocols, uris)
        .into_iter()
        .map(|a| {
            let whole = if a.vless {
                a.name.clone()
            } else {
                let mut whole = Vec::with_capacity(a.name.len() + a.value.len() + 3);
                write_whole(&mut whole, &a.name, &a.value);
                Cow::Owned(whole)
            };
            Attr {
                name: a.name,
                value: a.value,
                whole,
                vless: a.vless,
            }
        })
        .collect()
}

/// One attribute of [`hair_attrs`]: its name, its re-encoded value and whether it was written without one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HairAttr<'a> {
    pub name: Cow<'a, [u8]>,
    pub value: Cow<'a, [u8]>,
    pub vless: bool,
}

/// [`hair`] without each attribute's `whole`, which a caller can write itself ([`write_whole`]); borrows what it
/// does not change.
pub fn hair_attrs<'a>(
    attr: &'a [u8],
    protocols: &Protocols,
    uris: &UriAttributes,
) -> Vec<HairAttr<'a>> {
    let Some(parsed) = start_tag_attributes(attr) else {
        return Vec::new();
    };
    parsed
        .into_iter()
        .map(|a| match a.value {
            None => HairAttr {
                name: a.name,
                value: Cow::Borrowed(b""),
                vless: true,
            },
            Some(value) => {
                let value = if uris.0.iter().any(|u| **u == *a.name) {
                    match bad_protocol(&value, protocols) {
                        Cow::Borrowed(_) => value,
                        Cow::Owned(cleaned) => Cow::Owned(cleaned),
                    }
                } else {
                    value
                };
                HairAttr {
                    name: a.name,
                    value: recode(value),
                    vless: false,
                }
            }
        })
        .collect()
}

/// An attribute's `whole` as `wp_kses_hair()` writes it: `name="value"`.
pub fn write_whole(out: &mut Vec<u8>, name: &[u8], value: &[u8]) {
    out.extend_from_slice(name);
    out.extend_from_slice(b"=\"");
    out.extend_from_slice(value);
    out.push(b'"');
}

/// `strtr()` of HTML's five syntax characters to their references; borrows when there are none.
fn recode(value: Cow<'_, [u8]>) -> Cow<'_, [u8]> {
    if !value
        .iter()
        .any(|b| matches!(b, b'&' | b'<' | b'>' | b'\'' | b'"'))
    {
        return value;
    }
    let mut out = Vec::with_capacity(value.len() + 16);
    for b in value.iter() {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'\'' => out.extend_from_slice(b"&apos;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            b => out.push(*b),
        }
    }
    Cow::Owned(out)
}

/// Whether `$` matches at `i`.
fn at_dollar(s: &[u8], i: usize) -> bool {
    i == s.len() || (i + 1 == s.len() && s[i] == b'\n')
}

/// One match of `wp_kses_hair_parse()`'s attribute pattern at `at`: where it ends.
fn one_attribute(s: &[u8], at: usize) -> Option<usize> {
    let name = match s.get(at)? {
        b'[' => {
            let mut p = at + 1;
            if s.get(p) == Some(&b'[') {
                p += 1;
            }
            let body = s[p..]
                .iter()
                .take_while(|b| **b != b'[' && **b != b']')
                .count();
            if body == 0 || s.get(p + body) != Some(&b']') {
                return None;
            }
            p += body + 1;
            if s.get(p) == Some(&b']') {
                p += 1;
            }
            p - at
        }
        _ => attr_name(&s[at..]),
    };
    if name == 0 {
        return None;
    }
    let p = at + name;
    let value_end = (|| {
        let q = p + leading_space(&s[p..]);
        if s.get(q) != Some(&b'=') {
            return None;
        }
        let r = q + 1 + leading_space(&s[q + 1..]);
        match s.get(r)? {
            quote @ (b'"' | b'\'') => memchr::memchr(*quote, &s[r + 1..]).map(|e| r + 1 + e + 1),
            _ => {
                let bare = s[r..]
                    .iter()
                    .take_while(|b| !is_space(**b) && **b != b'"' && **b != b'\'')
                    .count();
                let end = r + bare;
                if bare == 0 {
                    None
                } else if s.get(end).is_some_and(|b| is_space(*b)) {
                    Some(end + 1)
                } else if at_dollar(s, end) {
                    Some(end)
                } else {
                    None
                }
            }
        }
    })();
    let end = match value_end {
        Some(end) => end,
        None if s.get(p).is_some_and(|b| is_space(*b)) => p + 1,
        None if at_dollar(s, p) => p,
        None => return None,
    };
    Some(end + leading_space(&s[end..]))
}

/// `wp_kses_hair_parse()`: the attribute list split into byte-exact pieces, or `None` when it does not parse.
pub fn hair_parse(attr: &[u8]) -> Option<Vec<&[u8]>> {
    let mut pieces = Vec::new();
    let mut at = 0;
    while at < attr.len() {
        let end = one_attribute(attr, at)?;
        if end == at {
            break;
        }
        pieces.push(&attr[at..end]);
        at = end;
    }
    if !at_dollar(attr, at) && at != attr.len() {
        return None;
    }
    Some(pieces)
}

/// `wp_kses_attr_parse()`: an element split into its opening (`<`, any `/`, name), its attributes and its closing
/// (`/` and `>`), every piece a slice of the input. `None` for a closing tag or anything that does not parse.
pub fn attr_parse(element: &[u8]) -> Option<Vec<&[u8]>> {
    if element.first() != Some(&b'<') {
        return None;
    }
    let mut p = 1 + leading_space(&element[1..]);
    if element.get(p) == Some(&b'/') {
        return None;
    }
    let name = element[p..]
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric())
        .count();
    if name == 0 {
        return None;
    }
    p += name;
    p += leading_space(&element[p..]);
    let opening = &element[..p];
    let attrs_end = memchr::memchr(b'>', &element[p..]).map_or(element.len(), |g| p + g);
    let closing_end = match &element[attrs_end..] {
        b"" => attrs_end,
        b">" | b">\n" => attrs_end + 1,
        _ => return None,
    };
    let attrs = &element[p..attrs_end];
    let slash = xhtml_slash_len(attrs);
    let mut pieces = vec![opening];
    pieces.extend(hair_parse(&attrs[..attrs.len() - slash])?);
    pieces.push(&element[attrs_end - slash..closing_end]);
    Some(pieces)
}

/// The length of the leftmost `\s*/\s*$` match at the end of `s`, 0 for none.
fn xhtml_slash_len(s: &[u8]) -> usize {
    let after = trailing_space(s);
    let slash_at = s.len() - after;
    if slash_at == 0 || s[slash_at - 1] != b'/' {
        return 0;
    }
    let before = trailing_space(&s[..slash_at - 1]);
    after + 1 + before
}
