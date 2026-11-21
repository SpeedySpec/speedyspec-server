//! One matcher per regex in kses.php, each over bytes.
//!
//! PCRE without flags: `\s` is space, `\t`, `\n`, `\v`, `\f`, `\r`; `.` excludes `\n`; `$` matches at the end of the
//! subject or before a final `\n`.

use std::ops::Range;

/// PCRE's non-UTF `\s`.
#[inline]
pub fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// Whether `$` matches at `i` in `s`.
#[inline]
fn at_dollar(s: &[u8], i: usize) -> bool {
    i == s.len() || (i + 1 == s.len() && s[i] == b'\n')
}

/// Which alternative of `wp_kses_split()`'s pattern matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    /// `<!--.*?(-->|$)`
    Comment,
    /// `</[^a-zA-Z][^>]*>`
    InvalidCloser,
    /// `<![^>]*>`
    Declaration,
    /// `<[^>]*(>|$)`
    Tag,
    /// `>`
    StrayGt,
}

/// The next match of `wp_kses_split()`'s token pattern at or after `from`: start, end (exclusive) and alternative.
#[inline]
pub fn next_token(s: &[u8], from: usize) -> Option<(usize, usize, TokenKind)> {
    let mut at = from;
    while at < s.len() {
        let p = at + memchr::memchr2(b'<', b'>', &s[at..])?;
        if s[p] == b'>' {
            return Some((p, p + 1, TokenKind::StrayGt));
        }
        if let Some(end) = comment_end(s, p) {
            return Some((p, end, TokenKind::Comment));
        }
        let next_gt = |from: usize| {
            memchr::memchr(b'>', s.get(from..).unwrap_or_default()).map(|i| from + i + 1)
        };
        if s.get(p + 1) == Some(&b'/') && s.get(p + 2).is_some_and(|c| !c.is_ascii_alphabetic()) {
            if let Some(end) = next_gt(p + 3) {
                return Some((p, end, TokenKind::InvalidCloser));
            }
        }
        if s.get(p + 1) == Some(&b'!') {
            if let Some(end) = next_gt(p + 2) {
                return Some((p, end, TokenKind::Declaration));
            }
        }
        return Some((p, next_gt(p + 1).unwrap_or(s.len()), TokenKind::Tag));
    }
    None
}

/// `<!--.*?(-->|$)` at `p`.
#[inline]
fn comment_end(s: &[u8], p: usize) -> Option<usize> {
    if !s[p..].starts_with(b"<!--") {
        return None;
    }
    let mut i = p + 4;
    loop {
        if s[i..].starts_with(b"-->") {
            return Some(i + 3);
        }
        if at_dollar(s, i) {
            return Some(i);
        }
        if s[i] == b'\n' {
            return None;
        }
        // Jump to the next byte that could end the lazy run.
        i += 1 + memchr::memchr2(b'-', b'\n', &s[i + 1..]).unwrap_or(s.len() - i - 1);
    }
}

/// The parts of `%^<\s*(/\s*)?([a-zA-Z0-9-]+)([^>]*)>?$%`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagHead {
    pub slash: bool,
    pub name: Range<usize>,
    pub attrs: Range<usize>,
}

/// `wp_kses_split2()`'s tag pattern.
#[inline]
pub fn tag_head(t: &[u8]) -> Option<TagHead> {
    if t.first() != Some(&b'<') {
        return None;
    }
    let mut i = 1 + leading_space(&t[1..]);
    let slash = t.get(i) == Some(&b'/');
    if slash {
        i += 1;
        i += leading_space(&t[i..]);
    }
    let name_start = i;
    while i < t.len() && (t[i].is_ascii_alphanumeric() || t[i] == b'-') {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let attrs_start = i;
    let attrs_end = memchr::memchr(b'>', &t[i..]).map_or(t.len(), |g| i + g);
    let rest = &t[attrs_end..];
    let ok = rest.is_empty() || rest == b">" || rest == b">\n";
    ok.then_some(TagHead {
        slash,
        name: name_start..attrs_start,
        attrs: attrs_start..attrs_end,
    })
}

/// `~^(?:</[^a-zA-Z][^>]*>|<![a-z][^>]*>)$~`.
#[inline]
pub fn invalid_wrapper(t: &[u8]) -> bool {
    let body_from = match t {
        [b'<', b'/', c, ..] if !c.is_ascii_alphabetic() => 3,
        [b'<', b'!', c, ..] if c.is_ascii_lowercase() => 3,
        _ => return false,
    };
    match memchr::memchr(b'>', &t[body_from..]) {
        Some(g) => at_dollar(t, body_from + g + 1),
        None => false,
    }
}

/// `^[_a-zA-Z][-_a-zA-Z0-9:.]*`: the length matched, 0 for none.
#[inline]
pub fn attr_name(s: &[u8]) -> usize {
    match s.first() {
        Some(c) if c.is_ascii_alphabetic() || *c == b'_' => {
            1 + s[1..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b':' | b'.'))
                .count()
        }
        _ => 0,
    }
}

/// `^\s*=\s*`: the length matched, 0 for none.
#[inline]
pub fn equals(s: &[u8]) -> usize {
    let lead = leading_space(s);
    if s.get(lead) != Some(&b'=') {
        return 0;
    }
    lead + 1 + leading_space(&s[lead + 1..])
}

/// `^\s*`: the length matched.
#[inline]
pub fn leading_space(s: &[u8]) -> usize {
    s.iter().take_while(|b| is_space(**b)).count()
}

/// `\s*$` (as `preg_match` finds it, leftmost): the length matched at the end.
#[inline]
pub fn trailing_space(s: &[u8]) -> usize {
    s.iter().rev().take_while(|b| is_space(**b)).count()
}

/// `%\s*/\s*$%`.
#[inline]
pub fn xhtml_slash(s: &[u8]) -> bool {
    let t = trailing_space(s);
    s.len() > t && s[s.len() - t - 1] == b'/'
}
