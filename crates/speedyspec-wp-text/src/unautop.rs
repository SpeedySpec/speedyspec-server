//! `shortcode_unautop()`.

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_wp_shortcode_contracts::{TagName, shortcode_tag_alternation};

use crate::preg::{Pattern, preg_replace};

/// The pattern 7.1.5's `shortcode_unautop()` builds from the tag names and the spaces fragment.
fn unautop_pattern(tags: &[TagName], spaces: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"/<p>(?:");
    p.extend_from_slice(spaces);
    p.extend_from_slice(b")*+(\\[(");
    p.extend_from_slice(&shortcode_tag_alternation(tags));
    p.extend_from_slice(
        br")(?![\w-])[^\]\/]*(?:\/(?!\])[^\]\/]*)*?(?:\/\]|\](?:[^\[]*+(?:\[(?!\/\2\])[^\[]*+)*+\[\/\2\])?))(?:",
    );
    p.extend_from_slice(spaces);
    p.extend_from_slice(br")*+<\/p>/");
    p
}

/// Whether some `[` follows `<p>` and default space tokens, which every match of the pattern needs.
fn has_candidate(text: &[u8]) -> bool {
    memchr::memchr_iter(b'[', text).any(|k| {
        let mut end = k;
        loop {
            let before = &text[..end];
            if matches!(before.last(), Some(b'\r' | b'\n' | b'\t' | b' ')) {
                end -= 1;
            } else if before.ends_with(b"\xC2\xA0") {
                end -= 2;
            } else if before.ends_with(b"&nbsp;") {
                end -= 6;
            } else {
                return before.ends_with(b"<p>");
            }
        }
    })
}

fn run<'a>(pattern: Option<&Pattern>, text: &'a [u8]) -> Cow<'a, [u8]> {
    // A pattern that doesn't compile, or a failed match, makes PHP's preg_replace return null: "" downstream.
    match pattern.map(|p| preg_replace(p, text, b"$1", None)) {
        Some(Ok((out, _))) => out,
        _ => Cow::Borrowed(&text[..0]),
    }
}

/// WordPress's `shortcode_unautop()`: removes the `<p>` that `wpautop()` wrapped around a shortcode standing alone.
///
/// This builds the pattern on every call; [`Unautop`] keeps it between calls.
///
/// @param text
///     The text.
/// @param tags
///     The registered shortcode names, in registration order.
/// @param spaces
///     `wp_spaces_regexp()`.
/// @return
///     The text, borrowed when there are no tags or nothing matched.
pub fn shortcode_unautop<'a>(text: &'a [u8], tags: &[TagName], spaces: &[u8]) -> Cow<'a, [u8]> {
    if tags.is_empty() || memchr::memchr(b'[', text).is_none() {
        return Cow::Borrowed(text);
    }
    run(Pattern::try_php(&unautop_pattern(tags, spaces)).ok().as_ref(), text)
}

/// `shortcode_unautop()` with its compiled pattern kept until the tags or the spaces fragment change.
#[derive(Default)]
pub struct Unautop {
    cache: RefCell<Option<(u64, Vec<u8>, Option<Rc<Pattern>>)>>,
}

impl Unautop {
    /// A new, empty cache.
    pub fn new() -> Unautop {
        Unautop::default()
    }

    /// `shortcode_unautop()`.
    ///
    /// @param text
    ///     The text.
    /// @param tags
    ///     The registered shortcode names, in registration order.
    /// @param generation
    ///     The registry's generation for `tags`: a new generation rebuilds the pattern.
    /// @param spaces
    ///     `wp_spaces_regexp()`.
    /// @return
    ///     The text, borrowed when there are no tags or nothing matched.
    pub fn run<'a>(&self, text: &'a [u8], tags: &[TagName], generation: u64, spaces: &[u8]) -> Cow<'a, [u8]> {
        // Every match has a `[` after `<p>` and spaces; text without one can't change.
        if tags.is_empty() || memchr::memchr(b'[', text).is_none() {
            return Cow::Borrowed(text);
        }
        if spaces == crate::small::DEFAULT_SPACES && !has_candidate(text) {
            return Cow::Borrowed(text);
        }
        let pattern = {
            let mut cache = self.cache.borrow_mut();
            match &*cache {
                Some((g, s, pattern)) if *g == generation && s == spaces => pattern.clone(),
                _ => {
                    let pattern = Pattern::try_php(&unautop_pattern(tags, spaces)).ok().map(Rc::new);
                    *cache = Some((generation, spaces.to_vec(), pattern.clone()));
                    pattern
                }
            }
        };
        run(pattern.as_deref(), text)
    }
}
