//! WordPress's `wp_html_split()`, in Rust.
//!
//! WordPress splits HTML into text and tags with a regular expression (`get_html_split_regex()`), not an HTML5
//! tokenizer, and several formatting functions (`wptexturize`, `wp_replace_in_html_tags`, `force_balance_tags`) and
//! the shortcode API depend on exactly how it treats malformed input. [`split`] reproduces that expression as a
//! scan over bytes that never allocates: an unterminated tag, comment or CDATA section runs to the end of the input,
//! and only `<!--` and `<![CDATA[` start an escaped element.

use memchr::memchr;
use memchr::memmem;

/// The pattern `get_html_split_regex()` returns, for PHP callers that use it directly.
pub const HTML_SPLIT_REGEX: &str = r"/(<(?(?=!--|!\[CDATA\[)(?(?=!-)!(?:-(?!->)[^\-]*+)*+(?:-->)?|!\[CDATA\[[^\]]*+(?:](?!]>)[^\]]*+)*+(?:]]>)?)|[^>]*>?))/";

/// One piece of split HTML.
///
/// A tag covers elements, comments and CDATA sections, as the expression's captures do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlChunk<'a> {
    Text(&'a [u8]),
    Tag(&'a [u8]),
}

impl<'a> HtmlChunk<'a> {
    /// The chunk's bytes, whichever kind it is.
    pub fn bytes(&self) -> &'a [u8] {
        match self {
            HtmlChunk::Text(bytes) | HtmlChunk::Tag(bytes) => bytes,
        }
    }
}

/// Which elements run to their own terminator instead of the next `>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Escapes {
    /// Comments (`-->`) and CDATA sections (`]]>`), as `get_html_split_regex()` has it.
    CommentsAndCdata,
    /// Comments only, as `_get_wptexturize_split_regex()` has it: `<![CDATA[` is an ordinary element.
    CommentsOnly,
}

/// Splits `input` into chunks, as `wp_html_split()` does.
///
/// It always yields text first and alternates, ending with text, so `n` tags give `2n + 1` chunks; text chunks may
/// be empty, as `preg_split` with `PREG_SPLIT_DELIM_CAPTURE` returns them.
pub fn split(input: &[u8]) -> Split<'_> {
    split_with(input, Escapes::CommentsAndCdata)
}

/// Splits `input` into chunks, with `escapes` deciding which elements run to their own terminator.
///
/// @param input
///     The HTML.
/// @param escapes
///     Whether CDATA sections are escaped as well as comments.
/// @return
///     The chunks, alternating text and tag as [`split`] yields them.
pub fn split_with(input: &[u8], escapes: Escapes) -> Split<'_> {
    Split {
        input,
        pos: 0,
        pending_tag: None,
        done: false,
        escapes,
    }
}

/// Where the tag that starts with the `<` at `start` ends (one past its last byte).
///
/// @param input
///     The HTML.
/// @param start
///     The position of a `<`.
/// @param escapes
///     Which elements run to their own terminator.
/// @return
///     The end of the tag; an unterminated one runs to the end of `input`.
pub fn tag_end(input: &[u8], start: usize, escapes: Escapes) -> usize {
    let rest = &input[start + 1..];
    let found = |needle: &[u8], from: usize| {
        memmem::find(&input[from..], needle).map(|at| from + at + needle.len())
    };
    if rest.starts_with(b"!--") {
        // The comment search starts at the first dash, which is why `<!-->` is a whole comment.
        found(b"-->", start + 2).unwrap_or(input.len())
    } else if escapes == Escapes::CommentsAndCdata && rest.starts_with(b"![CDATA[") {
        found(b"]]>", start + 9).unwrap_or(input.len())
    } else {
        memchr(b'>', rest).map_or(input.len(), |at| start + 1 + at + 1)
    }
}

/// `wp_html_split()`: the chunks' bytes in order.
pub fn wp_html_split(input: &[u8]) -> Vec<&[u8]> {
    split(input).map(|chunk| chunk.bytes()).collect()
}

/// The iterator [`split`] returns.
#[derive(Clone, Debug)]
pub struct Split<'a> {
    input: &'a [u8],
    pos: usize,
    pending_tag: Option<(usize, usize)>,
    done: bool,
    escapes: Escapes,
}

impl<'a> Iterator for Split<'a> {
    type Item = HtmlChunk<'a>;

    fn next(&mut self) -> Option<HtmlChunk<'a>> {
        if let Some((start, end)) = self.pending_tag.take() {
            self.pos = end;
            return Some(HtmlChunk::Tag(&self.input[start..end]));
        }
        if self.done {
            return None;
        }
        let from = self.pos;
        match memchr(b'<', &self.input[from..]) {
            Some(at) => {
                let start = from + at;
                self.pending_tag = Some((start, tag_end(self.input, start, self.escapes)));
                Some(HtmlChunk::Text(&self.input[from..start]))
            }
            None => {
                self.done = true;
                Some(HtmlChunk::Text(&self.input[from..]))
            }
        }
    }
}
