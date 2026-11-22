//! PHP's `strip_tags()` and WordPress's `wp_strip_all_tags()`.

use std::borrow::Cow;
use std::sync::LazyLock;

use crate::preg::{Pattern, preg_replace};
use crate::strings::{PHP_TRIM_DEFAULT, php_trim};

/// C's `isspace()` in the "C" locale.
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// Where `strip_tags()`'s state machine is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    /// Text.
    Text,
    /// Inside a tag.
    Tag,
    /// Inside `<?` (PHP code).
    Php,
    /// Inside `<!` (a declaration).
    Bang,
    /// Inside `<!--` (a comment).
    Comment,
}

/// PHP's `strip_tags()` with no allowed tags.
///
/// A line-by-line port of `php_strip_tags_ex()` (PHP 8.5.12, `ext/standard/string.c`) with `allow` empty and
/// `allow_tag_spaces` false, keeping its quirks: a `<` followed by whitespace is text, quotes inside a tag hide
/// `>`, `<?xml` switches back to tag parsing, `<!DOCTYPE` ends like a tag, NUL bytes are dropped, and an unclosed
/// tag swallows the rest of the input.
///
/// @param text
///     The text.
/// @return
///     The text without tags, borrowed when it has neither `<`, `>` nor NUL.
pub fn strip_tags(text: &[u8]) -> Cow<'_, [u8]> {
    if !text.iter().any(|&b| b == b'<' || b == b'>' || b == 0) {
        return Cow::Borrowed(text);
    }
    let at = |i: usize| text.get(i).copied().unwrap_or(0);
    let mut out = Vec::with_capacity(text.len());
    let mut state = State::Text;
    let mut depth = 0usize;
    let mut in_q = 0u8;
    let mut lc = 0u8;
    let mut br = 0i64;
    let mut is_xml = false;
    let mut p = 0usize;
    while p < text.len() {
        let c = text[p];
        let prev = |n: usize| if p >= n { text[p - n] } else { 0 };
        match state {
            State::Text => match c {
                0 => {}
                b'<' => {
                    if in_q != 0 {
                    } else if is_c_space(at(p + 1)) {
                        out.push(c);
                    } else {
                        lc = b'<';
                        state = State::Tag;
                    }
                }
                b'>' => {
                    if depth > 0 {
                        depth -= 1;
                    } else if in_q == 0 {
                        out.push(c);
                    }
                }
                _ => out.push(c),
            },
            State::Tag => match c {
                0 => {}
                b'<' => {
                    if in_q == 0 && !is_c_space(at(p + 1)) {
                        depth += 1;
                    }
                }
                b'>' => {
                    if depth > 0 {
                        depth -= 1;
                    } else if in_q == 0 {
                        lc = b'>';
                        if !(is_xml && p >= 1 && prev(1) == b'-') {
                            in_q = 0;
                            is_xml = false;
                            state = State::Text;
                        }
                    }
                }
                b'"' | b'\'' => {
                    if p != 0 && (in_q == 0 || c == in_q) {
                        in_q = if in_q != 0 { 0 } else { c };
                    }
                }
                b'!' if p >= 1 && prev(1) == b'<' => {
                    state = State::Bang;
                    lc = c;
                }
                b'?' if p >= 1 && prev(1) == b'<' => {
                    br = 0;
                    state = State::Php;
                }
                _ => {}
            },
            State::Php => match c {
                b'(' => {
                    if lc != b'"' && lc != b'\'' {
                        lc = b'(';
                        br += 1;
                    }
                }
                b')' => {
                    if lc != b'"' && lc != b'\'' {
                        lc = b')';
                        br -= 1;
                    }
                }
                b'>' => {
                    if depth > 0 {
                        depth -= 1;
                    } else if in_q == 0 && br == 0 && p >= 1 && lc != b'"' && prev(1) == b'?' {
                        in_q = 0;
                        state = State::Text;
                    }
                }
                b'"' | b'\'' => {
                    if p >= 1 && prev(1) != b'\\' {
                        if lc == c {
                            lc = 0;
                        } else if lc != b'\\' {
                            lc = c;
                        }
                        if p != 0 && (in_q == 0 || c == in_q) {
                            in_q = if in_q != 0 { 0 } else { c };
                        }
                    }
                }
                b'l' | b'L'
                    if p > 4
                        && prev(1).eq_ignore_ascii_case(&b'm')
                        && prev(2).eq_ignore_ascii_case(&b'x')
                        && prev(3) == b'?'
                        && prev(4) == b'<' =>
                {
                    state = State::Tag;
                    is_xml = true;
                }
                _ => {}
            },
            State::Bang => match c {
                b'>' => {
                    if depth > 0 {
                        depth -= 1;
                    } else if in_q == 0 {
                        in_q = 0;
                        state = State::Text;
                    }
                }
                b'"' | b'\'' => {
                    if p != 0 && prev(1) != b'\\' && (in_q == 0 || c == in_q) {
                        in_q = if in_q != 0 { 0 } else { c };
                    }
                }
                b'-' if p >= 2 && prev(1) == b'-' && prev(2) == b'!' => state = State::Comment,
                b'E' | b'e'
                    if p > 6
                        && prev(1).eq_ignore_ascii_case(&b'p')
                        && prev(2).eq_ignore_ascii_case(&b'y')
                        && prev(3).eq_ignore_ascii_case(&b't')
                        && prev(4).eq_ignore_ascii_case(&b'c')
                        && prev(5).eq_ignore_ascii_case(&b'o')
                        && prev(6).eq_ignore_ascii_case(&b'd') =>
                {
                    state = State::Tag;
                }
                _ => {}
            },
            State::Comment => {
                if c == b'>' && in_q == 0 && p >= 2 && prev(1) == b'-' && prev(2) == b'-' {
                    in_q = 0;
                    state = State::Text;
                }
            }
        }
        p += 1;
    }
    Cow::Owned(out)
}

// formatting.php, wp_strip_all_tags().
static SCRIPT_STYLE: LazyLock<Pattern> = LazyLock::new(|| Pattern::php("@<(script|style)[^>]*?>.*?</\\1>@si"));

/// WordPress's `wp_strip_all_tags()` for a string: drops `<script>` and `<style>` elements with their contents,
/// strips the remaining tags, optionally collapses whitespace, and trims.
///
/// The `null`, scalar and non-scalar handling is the facade's.
///
/// @param text
///     The text.
/// @param remove_breaks
///     Whether runs of whitespace become one space.
/// @return
///     The plain text.
pub fn wp_strip_all_tags_core(text: &[u8], remove_breaks: bool) -> Cow<'_, [u8]> {
    // The element pattern needs `<script` or `<style` (any case).
    let opens_element = memchr::memchr_iter(b'<', text).any(|at| {
        let rest = &text[at + 1..];
        rest.get(..6).is_some_and(|w| w.eq_ignore_ascii_case(b"script")) || rest.get(..5).is_some_and(|w| w.eq_ignore_ascii_case(b"style"))
    });
    let without_scripts = if !opens_element {
        Cow::Borrowed(text)
    } else {
        match preg_replace(&SCRIPT_STYLE, text, b"", None) {
            Ok((out, _)) => out,
            Err(_) => Cow::Borrowed(&text[..0]),
        }
    };
    let stripped = match without_scripts {
        Cow::Borrowed(b) => strip_tags(b),
        Cow::Owned(o) => Cow::Owned(strip_tags(&o).into_owned()),
    };
    // `/[\r\n\t ]+/` to one space.
    let collapsed = if remove_breaks {
        Cow::Owned(crate::sanitize::collapse_runs(&stripped, |b| matches!(b, b'\r' | b'\n' | b'\t' | b' ')))
    } else {
        stripped
    };
    match collapsed {
        Cow::Borrowed(b) => Cow::Borrowed(php_trim(b, PHP_TRIM_DEFAULT)),
        Cow::Owned(o) => Cow::Owned(php_trim(&o, PHP_TRIM_DEFAULT).to_vec()),
    }
}
