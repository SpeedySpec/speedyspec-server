//! WordPress's `normalize_whitespace()`.
//!
//! The text formatting port has the same function (`speedyspec_wp_text::normalize_whitespace`), but that crate links
//! its own PCRE2, whose internal symbols clash with the PCRE2 inside the static PHP: linked into a binary that embeds
//! PHP, it breaks every `preg_*` call. Until that is fixed, the diff carries this port, which needs no regex.

use std::borrow::Cow;

use speedyspec_wp_text_diff_contracts::WhitespaceNormalizer;

/// PHP's `trim()` default characters: `" \t\n\r\0\x0B"`.
fn is_trimmed(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\0' | 0x0B)
}

/// WordPress's `normalize_whitespace()`: `trim()`, `\r` to `\n`, then `/\n+/` to `"\n"` and `/[ \t]+/` to `" "`.
///
/// @param text
///     The text.
/// @return
///     The normalized text, borrowed (possibly trimmed) when nothing else changes.
pub fn normalize_whitespace(text: &[u8]) -> Cow<'_, [u8]> {
    let start = text
        .iter()
        .position(|&b| !is_trimmed(b))
        .unwrap_or(text.len());
    let end = text
        .iter()
        .rposition(|&b| !is_trimmed(b))
        .map_or(start, |i| i + 1);
    let trimmed = &text[start..end];
    let needs_work = trimmed.iter().any(|&b| b == b'\r' || b == b'\t')
        || trimmed
            .windows(2)
            .any(|w| (w[0] == b'\n' && w[1] == b'\n') || (w[0] == b' ' && w[1] == b' '));
    if !needs_work {
        return Cow::Borrowed(trimmed);
    }
    let mut out = Vec::with_capacity(trimmed.len());
    for &b in trimmed {
        match b {
            b'\n' | b'\r' if out.last() == Some(&b'\n') => {}
            b'\n' | b'\r' => out.push(b'\n'),
            b' ' | b'\t' if out.last() == Some(&b' ') => {}
            b' ' | b'\t' => out.push(b' '),
            other => out.push(other),
        }
    }
    Cow::Owned(out)
}

/// [`WhitespaceNormalizer`] as WordPress's `normalize_whitespace()`.
#[derive(Clone, Copy, Debug, Default)]
pub struct WordPressWhitespace;

impl WhitespaceNormalizer for WordPressWhitespace {
    fn normalize<'a>(&self, text: &'a [u8]) -> Cow<'a, [u8]> {
        normalize_whitespace(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// WordPress's three steps, one after another, as PHP runs them.
    fn reference(text: &[u8]) -> Vec<u8> {
        let start = text
            .iter()
            .position(|&b| !is_trimmed(b))
            .unwrap_or(text.len());
        let end = text
            .iter()
            .rposition(|&b| !is_trimmed(b))
            .map_or(start, |i| i + 1);
        let s: Vec<u8> = text[start..end]
            .iter()
            .map(|&b| if b == b'\r' { b'\n' } else { b })
            .collect();
        let mut lines = Vec::new();
        for &b in &s {
            if !(b == b'\n' && lines.last() == Some(&b'\n')) {
                lines.push(b);
            }
        }
        let mut out: Vec<u8> = Vec::new();
        let mut in_run = false;
        for &b in &lines {
            if b == b' ' || b == b'\t' {
                if !in_run {
                    out.push(b' ');
                }
                in_run = true;
            } else {
                out.push(b);
                in_run = false;
            }
        }
        out
    }

    #[test]
    fn normalizes_as_wordpress_does() {
        assert_eq!(&*normalize_whitespace(b"  a\r\n\n\nb \t c\t\n"), b"a\nb c");
        assert_eq!(&*normalize_whitespace(b"\0\x0Ba\x0Bb\0"), b"a\x0Bb");
        assert_eq!(&*normalize_whitespace(b"a\n \nb"), b"a\n \nb");
        assert_eq!(&*normalize_whitespace(b""), b"");
        assert!(matches!(
            normalize_whitespace(b" trimmed "),
            Cow::Borrowed(b"trimmed")
        ));
        assert_eq!(&*WordPressWhitespace.normalize(b"a\t\tb"), b"a b");
    }

    proptest! {
        #[test]
        fn matches_the_three_steps(v in proptest::collection::vec(
            prop_oneof![Just(b' '), Just(b'\t'), Just(b'\n'), Just(b'\r'), Just(b'\0'), Just(0x0Bu8), any::<u8>()],
            0..64,
        )) {
            prop_assert_eq!(normalize_whitespace(&v).into_owned(), reference(&v));
        }
    }
}
