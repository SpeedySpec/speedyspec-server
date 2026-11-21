//! `wptexturize()`: curly quotes, dashes, ellipses and the rest, outside tags, shortcodes and code.

use std::borrow::Cow;

use speedyspec_wp_shortcode_contracts::{TagName, shortcode_tag_alternation};

use crate::preg::PregError;

mod pcre;
mod primes;
mod tables;

pub use pcre::PcreTexturizer;
pub use tables::TexturizeTables;

/// What one `wptexturize()` call reads besides its text: the filtered lists of elements and shortcodes to leave
/// alone, and the registered shortcode names.
pub struct TexturizeScope<'s> {
    /// `no_texturize_tags`, after its filter: string entries only.
    pub no_texturize_tags: &'s [Vec<u8>],
    /// `no_texturize_shortcodes`, after its filter: string entries only.
    pub no_texturize_shortcodes: &'s [Vec<u8>],
    /// The registered shortcodes, in registration order.
    pub shortcode_tags: &'s [TagName],
}

/// A way of running `wptexturize()` over prepared tables.
///
/// [`PcreTexturizer`] runs WordPress's own patterns and is the reference; a faster implementation must give the
/// same bytes.
pub trait Texturizer {
    /// Texturizes `text`.
    ///
    /// @param text
    ///     The text.
    /// @param tables
    ///     The translated replacement tables.
    /// @param scope
    ///     The lists and shortcode names for this call.
    /// @return
    ///     The texturized text, borrowed when nothing changed; an error when a pattern built from a filtered
    ///     spaces fragment fails.
    fn texturize<'a>(
        &self,
        text: &'a [u8],
        tables: &TexturizeTables,
        scope: &TexturizeScope<'_>,
    ) -> Result<Cow<'a, [u8]>, PregError>;
}

/// `_get_wptexturize_shortcode_regex()`: a pattern (no delimiters) for the registered shortcodes among `tags`.
///
/// @param tags
///     The shortcode names found in the text, in registration order.
/// @return
///     The pattern source.
pub fn texturize_shortcode_regex(tags: &[TagName]) -> Vec<u8> {
    let mut regex = br"\[[\/\[]?(?:".to_vec();
    regex.extend_from_slice(&shortcode_tag_alternation(tags));
    regex.extend_from_slice(br")(?=[\s\]\/])(?:[^\[\]<>]+|<[^\[\]>]*>)*+\]\]?");
    regex
}

/// `_wptexturize_pushpop_element()`: keeps the stack of open elements (or shortcodes) whose content is left alone.
pub(crate) fn pushpop_element(text: &[u8], stack: &mut Vec<Vec<u8>>, disabled: &[Vec<u8>]) {
    let (opening, offset) = if text.len() > 1 && text[1] != b'/' {
        (true, 1)
    } else if stack.is_empty() {
        return;
    } else {
        (false, 2)
    };
    let length = match memchr::memchr(b' ', text) {
        Some(space) => space as i64 - offset,
        None => -1,
    };
    let tag = speedyspec_wp_utf8::php_substr(text, offset, Some(length));
    if disabled.iter().any(|d| d == tag) {
        if opening {
            stack.push(tag.to_vec());
        } else if stack.last().is_some_and(|last| last == tag) {
            stack.pop();
        }
    }
}
