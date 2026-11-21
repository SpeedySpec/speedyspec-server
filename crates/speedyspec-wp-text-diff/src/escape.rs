//! `htmlspecialchars()` with PHP 8.1+'s defaults: `ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401`, UTF-8, double encoding.

use std::borrow::Cow;

use crate::simd::find_escaped;

const REPLACEMENT: &[u8] = "\u{FFFD}".as_bytes();

fn entity(b: u8) -> &'static [u8] {
    match b {
        b'&' => b"&amp;",
        b'"' => b"&quot;",
        b'\'' => b"&#039;",
        b'<' => b"&lt;",
        _ => b"&gt;",
    }
}

/// Escapes `s` as `htmlspecialchars( $s )` does.
pub fn html_special_chars(s: &[u8]) -> Cow<'_, [u8]> {
    if simdutf8::basic::from_utf8(s).is_ok() && find_escaped(s).is_none() {
        return Cow::Borrowed(s);
    }
    let mut out = Vec::with_capacity(s.len() + s.len() / 8 + 8);
    escape_into(&mut out, s);
    Cow::Owned(out)
}

/// Appends `htmlspecialchars( $s )` to `out`.
pub fn escape_into(out: &mut Vec<u8>, s: &[u8]) {
    if simdutf8::basic::from_utf8(s).is_ok() {
        escape_valid(out, s);
    } else {
        escape_substituting(out, s);
    }
}

/// Appends `htmlspecialchars( $s )` for `s` already known to be valid UTF-8: copy clean runs whole, an entity for
/// each special byte.
pub(crate) fn escape_valid(out: &mut Vec<u8>, mut s: &[u8]) {
    while let Some(i) = find_escaped(s) {
        out.extend_from_slice(&s[..i]);
        out.extend_from_slice(entity(s[i]));
        s = &s[i + 1..];
    }
    out.extend_from_slice(s);
}

fn lead(b: u8) -> bool {
    b < 0x80 || (0xC2..=0xF4).contains(&b)
}

fn trail(b: u8) -> bool {
    (0x80..=0xBF).contains(&b)
}

/// Invalid UTF-8: PHP's `get_next_char()` for UTF-8, which replaces each bad sequence with U+FFFD and decides how many
/// bytes the bad sequence spans.
fn escape_substituting(out: &mut Vec<u8>, s: &[u8]) {
    let at = |i: usize| s.get(i).copied();
    let mut pos = 0;
    while pos < s.len() {
        let c = s[pos];
        let avail = s.len() - pos;
        let step = if c < 0x80 {
            if crate::simd::is_escaped(c) {
                out.extend_from_slice(entity(c));
            } else {
                out.push(c);
            }
            pos += 1;
            continue;
        } else if c < 0xC2 {
            Err(1)
        } else if c < 0xE0 {
            if avail < 2 {
                Err(1)
            } else if !trail(s[pos + 1]) {
                Err(if lead(s[pos + 1]) { 1 } else { 2 })
            } else {
                Ok(2)
            }
        } else if c < 0xF0 {
            let ok = avail >= 3 && trail(s[pos + 1]) && trail(s[pos + 2]);
            if !ok {
                if avail < 2 || lead(s[pos + 1]) {
                    Err(1)
                } else if avail < 3 || lead(s[pos + 2]) {
                    Err(2)
                } else {
                    Err(3)
                }
            } else {
                let cp = ((c as u32 & 0x0F) << 12)
                    | ((s[pos + 1] as u32 & 0x3F) << 6)
                    | (s[pos + 2] as u32 & 0x3F);
                if cp < 0x800 || (0xD800..=0xDFFF).contains(&cp) {
                    Err(3)
                } else {
                    Ok(3)
                }
            }
        } else if c < 0xF5 {
            let ok = avail >= 4 && trail(s[pos + 1]) && trail(s[pos + 2]) && trail(s[pos + 3]);
            if !ok {
                if avail < 2 || lead(s[pos + 1]) {
                    Err(1)
                } else if avail < 3 || lead(s[pos + 2]) {
                    Err(2)
                } else if avail < 4 || at(pos + 3).is_some_and(lead) {
                    Err(3)
                } else {
                    Err(4)
                }
            } else {
                let cp = ((c as u32 & 0x07) << 18)
                    | ((s[pos + 1] as u32 & 0x3F) << 12)
                    | ((s[pos + 2] as u32 & 0x3F) << 6)
                    | (s[pos + 3] as u32 & 0x3F);
                if !(0x10000..=0x10FFFF).contains(&cp) {
                    Err(4)
                } else {
                    Ok(4)
                }
            }
        } else {
            Err(1)
        };
        match step {
            Ok(n) => {
                out.extend_from_slice(&s[pos..pos + n]);
                pos += n;
            }
            Err(n) => {
                out.extend_from_slice(REPLACEMENT);
                pos += n;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_the_five_characters() {
        assert_eq!(
            &*html_special_chars(b"<a href=\"x\">Tom & 'J'</a>"),
            b"&lt;a href=&quot;x&quot;&gt;Tom &amp; &#039;J&#039;&lt;/a&gt;"
        );
    }

    #[test]
    fn encodes_entities_again() {
        assert_eq!(&*html_special_chars(b"&amp;"), b"&amp;amp;");
    }

    #[test]
    fn borrows_clean_text() {
        assert!(matches!(
            html_special_chars("café".as_bytes()),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn substitutes_invalid_utf8() {
        assert_eq!(
            &*html_special_chars(b"a\xffb'"),
            "a\u{FFFD}b&#039;".as_bytes()
        );
    }
}
