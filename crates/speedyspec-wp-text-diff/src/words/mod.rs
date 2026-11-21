//! WordPress's `_splitOnWords()`: `preg_split( '/([^\w])/u', $string, -1, PREG_SPLIT_DELIM_CAPTURE )` after removing
//! NUL bytes.

mod table;

use std::borrow::Cow;
use std::ops::Range;

use crate::simd::{ascii_word_run, is_ascii_word};

/// Whether `cp` is a character PCRE's `\w` matches under `/u`.
pub fn is_word_char(cp: u32) -> bool {
    if cp < 0x80 {
        return is_ascii_word(cp as u8);
    }
    table::WORD_RANGES
        .binary_search_by(|&(lo, hi)| {
            if hi < cp {
                std::cmp::Ordering::Less
            } else if lo > cp {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// A line split into words and the single characters between them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Words<'a> {
    text: Cow<'a, [u8]>,
    pieces: Vec<Range<usize>>,
}

impl<'a> Words<'a> {
    /// The pieces, in order: a word (possibly empty), then a delimiter character, and so on, ending with a word.
    pub fn pieces(&self) -> impl ExactSizeIterator<Item = &[u8]> + '_ {
        self.pieces.iter().map(|r| &self.text[r.clone()])
    }

    /// The pieces as slices, for a differ.
    pub fn slices(&self) -> Vec<&[u8]> {
        self.pieces().collect()
    }
}

/// Splits `s` as WordPress's `WP_Text_Diff_Renderer_inline::_splitOnWords()` does.
///
/// `None` when `s` (without its NUL bytes) is not valid UTF-8, where `preg_split()` returns `false`.
pub fn split_words(s: &[u8]) -> Option<Words<'_>> {
    let text: Cow<'_, [u8]> = if memchr::memchr(0, s).is_some() {
        Cow::Owned(s.iter().copied().filter(|&b| b != 0).collect())
    } else {
        Cow::Borrowed(s)
    };
    let valid = simdutf8::basic::from_utf8(&text).ok()?;
    let bytes = valid.as_bytes();
    let mut pieces = Vec::with_capacity(bytes.len() / 3 + 1);
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        i += ascii_word_run(&bytes[i..]);
        if i >= bytes.len() {
            break;
        }
        let b = bytes[i];
        let (cp, len) = if b < 0x80 {
            (b as u32, 1)
        } else {
            decode(&bytes[i..])
        };
        if cp >= 0x80 && is_word_char(cp) {
            i += len;
            continue;
        }
        pieces.push(start..i);
        pieces.push(i..i + len);
        i += len;
        start = i;
    }
    pieces.push(start..bytes.len());
    Some(Words { text, pieces })
}

/// Decodes one character of valid UTF-8.
fn decode(s: &[u8]) -> (u32, usize) {
    let b = s[0];
    if b < 0xE0 {
        ((((b & 0x1F) as u32) << 6) | (s[1] & 0x3F) as u32, 2)
    } else if b < 0xF0 {
        (
            (((b & 0x0F) as u32) << 12) | (((s[1] & 0x3F) as u32) << 6) | (s[2] & 0x3F) as u32,
            3,
        )
    } else {
        (
            (((b & 0x07) as u32) << 18)
                | (((s[1] & 0x3F) as u32) << 12)
                | (((s[2] & 0x3F) as u32) << 6)
                | (s[3] & 0x3F) as u32,
            4,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(s: &str) -> Vec<String> {
        split_words(s.as_bytes())
            .unwrap()
            .pieces()
            .map(|p| String::from_utf8(p.to_vec()).unwrap())
            .collect()
    }

    #[test]
    fn keeps_delimiters_and_empty_words() {
        assert_eq!(split("a  b"), ["a", " ", "", " ", "b"]);
        assert_eq!(split(""), [""]);
        assert_eq!(split("x."), ["x", ".", ""]);
    }

    #[test]
    fn unicode_letters_and_marks_are_word_characters() {
        assert_eq!(split("café x\u{0301}y"), ["café", " ", "x\u{0301}y"]);
        assert_eq!(split("a—b"), ["a", "—", "b"]);
    }

    #[test]
    fn removes_nul_bytes_first() {
        assert_eq!(split("a\0b c"), ["ab", " ", "c"]);
    }

    #[test]
    fn rejects_invalid_utf8() {
        assert!(split_words(b"a\xffb").is_none());
    }
}
