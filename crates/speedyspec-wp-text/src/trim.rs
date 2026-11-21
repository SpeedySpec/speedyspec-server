//! `wp_trim_words()` and `wp_html_excerpt()`.

use std::sync::LazyLock;

use crate::preg::{Pattern, SplitFlags, preg_match_all_group, preg_replace, preg_split};
use crate::strings::{PHP_TRIM_DEFAULT, php_trim};
use crate::strip::wp_strip_all_tags_core;

// formatting.php, wp_trim_words().
static WHITESPACE: LazyLock<Pattern> = LazyLock::new(|| Pattern::php("/[\\n\\r\\t ]+/"));
static CHARACTER: LazyLock<Pattern> = LazyLock::new(|| Pattern::php("/./u"));
// formatting.php, wp_html_excerpt().
static PARTIAL_ENTITY: LazyLock<Pattern> = LazyLock::new(|| Pattern::php("/&[^;\\s]{0,6}$/"));

/// PHP's `array_slice($list, 0, $length)`: a negative length stops that many items from the end.
fn slice_front<T>(mut list: Vec<T>, length: i64) -> Vec<T> {
    let keep = if length >= 0 { (length as usize).min(list.len()) } else { list.len().saturating_sub(length.unsigned_abs() as usize) };
    list.truncate(keep);
    list
}

/// `wp_trim_words()` without its filter: the text stripped of tags and cut to `num_words` words (or characters).
///
/// @param text
///     The text.
/// @param num_words
///     How many words to keep, after PHP's `(int)` cast. Zero and below keep PHP's limit arithmetic.
/// @param more
///     What to append when the text was cut.
/// @param count_characters
///     Whether the word count type starts with `characters` and the charset is UTF-8, so characters are counted.
/// @return
///     The trimmed text.
pub fn wp_trim_words_core(text: &[u8], num_words: i64, more: &[u8], count_characters: bool) -> Vec<u8> {
    let text = wp_strip_all_tags_core(text, false);
    let limit = num_words.saturating_add(1);
    let collapsed;
    let (mut words, sep): (Vec<&[u8]>, &[u8]) = if count_characters {
        collapsed = preg_replace(&WHITESPACE, &text, b" ", None).map_or_else(|_| text.clone(), |(out, _)| out);
        // A failed /./u match (invalid UTF-8) leaves PHP with no characters.
        let characters = preg_match_all_group(&CHARACTER, php_trim(&collapsed, b" "), 0).unwrap_or_default();
        (slice_front(characters, limit), b"")
    } else {
        let flags = SplitFlags { delim_capture: false, no_empty: true };
        (preg_split(&WHITESPACE, &text, limit, flags).unwrap_or_default(), b" ")
    };
    let cut = words.len() as i64 > num_words;
    if cut {
        words.pop();
    }
    let mut out = words.join(sep);
    if cut {
        out.extend_from_slice(more);
    }
    out
}

/// WordPress's `wp_html_excerpt()`: the text stripped of tags, cut to `count` characters, without a partial
/// entity at the end, with `more` appended when it was cut.
///
/// @param text
///     The text.
/// @param count
///     How many characters to keep.
/// @param more
///     What to append when the text was cut.
/// @return
///     The excerpt.
pub fn wp_html_excerpt(text: &[u8], count: i64, more: &[u8]) -> Vec<u8> {
    let stripped = wp_strip_all_tags_core(text, true);
    let excerpt = mbstring_substr_utf8(&stripped, count);
    let excerpt = preg_replace(&PARTIAL_ENTITY, &excerpt, b"", None).map_or_else(|_| Vec::new(), |(out, _)| out.into_owned());
    if *stripped != excerpt[..] {
        let mut out = php_trim(&excerpt, PHP_TRIM_DEFAULT).to_vec();
        out.extend_from_slice(more);
        return out;
    }
    excerpt
}

/// PHP's `mb_substr($s, 0, $length)` from the mbstring extension, on UTF-8.
///
/// mbstring converts as it cuts: each maximal invalid UTF-8 subpart counts as one character and comes out as `?`.
/// (WordPress's own `_mb_substr()` fallback, which `speedyspec-wp-utf8` ports, doesn't; the recorder's PHP has
/// mbstring.) A negative length stops that many characters from the end.
fn mbstring_substr_utf8(s: &[u8], length: i64) -> Vec<u8> {
    let mut units: Vec<&[u8]> = Vec::new();
    for chunk in s.utf8_chunks() {
        let valid = chunk.valid();
        let mut i = 0;
        for c in valid.chars() {
            units.push(&valid.as_bytes()[i..i + c.len_utf8()]);
            i += c.len_utf8();
        }
        if !chunk.invalid().is_empty() {
            units.push(b"?");
        }
    }
    slice_front(units, length).concat()
}
