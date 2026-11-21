//! The HTML5 named character references, as WordPress's precomputed table holds them, and the lookup the HTML
//! decoder makes.

use crate::html5_table::{GROUPS, KEY_LENGTH, LARGE_WORDS, SMALL_MAPPINGS, SMALL_WORDS};
use crate::{CaseSensitivity, TokenMapView, Words};

/// WordPress's `$html5_named_character_references`: the table as compile-time data, with no work at run time.
pub fn html5() -> TokenMapView<'static> {
    TokenMapView {
        key_length: KEY_LENGTH,
        groups: GROUPS,
        large_words: Words::Static(LARGE_WORDS),
        small_words: SMALL_WORDS,
        small_mappings: Words::Static(SMALL_MAPPINGS),
    }
}

/// The replacement for the named character reference at `offset` (the name without its `&`), and the name's byte
/// length: `$html5_named_character_references->read_token( $text, $offset, $length )`, case-sensitive, longest
/// name first, with `read_token()`'s NUL and short-text rules.
pub fn named_reference(text: &[u8], offset: usize) -> Option<(&'static [u8], usize)> {
    html5().read_token(text, offset, CaseSensitivity::CaseSensitive)
}

/// How many names the table holds (2,231).
pub fn named_reference_count() -> usize {
    let groups: usize = LARGE_WORDS
        .iter()
        .map(|group| {
            let (mut at, mut n) = (0, 0);
            while at < group.len() {
                at += 1 + group[at] as usize;
                at += 1 + group[at] as usize;
                n += 1;
            }
            n
        })
        .sum();
    groups + SMALL_MAPPINGS.len()
}
