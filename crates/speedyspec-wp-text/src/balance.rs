//! `force_balance_tags()`.

use std::borrow::Cow;
use std::sync::LazyLock;

use memchr::memmem;

use crate::preg::{Pattern, preg_match, preg_replace};
use crate::strings::{PHP_TRIM_DEFAULT, php_trim, replace_one};

const SINGLE_TAGS: &[&[u8]] = &[
    b"area", b"base", b"basefont", b"br", b"col", b"command", b"embed", b"frame", b"hr", b"img", b"input", b"isindex",
    b"link", b"meta", b"param", b"source", b"track", b"wbr",
];
const NESTABLE_TAGS: &[&[u8]] =
    &[b"article", b"aside", b"blockquote", b"details", b"div", b"figure", b"object", b"q", b"section", b"span"];

// formatting.php, force_balance_tags().
static DIGIT_TAG: LazyLock<Pattern> = LazyLock::new(|| Pattern::php("#<([0-9]{1})#"));
static TAG: LazyLock<Pattern> = LazyLock::new(|| {
    Pattern::php(r"#<(/?)((?:[a-z](?:[a-z0-9._]*)-(?:[a-z0-9._-]+)+)|(?:[\w:]+))(?:\s*(/?)|(\s+)([^>]*))>#")
});

/// WordPress's `force_balance_tags()`: closes open tags, drops stray closing tags, and self-closes single tags,
/// with WordPress's quirks (`<1` becomes `&lt;1`, `< !--` survives, a nested non-nestable tag closes the one before).
///
/// @param text
///     The HTML.
/// @return
///     The balanced HTML.
pub fn force_balance_tags(text: &[u8]) -> Cow<'_, [u8]> {
    let mut text = replace_one(text, b"< !--", b"<    !--").into_owned();
    if let Ok((Cow::Owned(out), _)) = preg_replace(&DIGIT_TAG, &text, b"&lt;$1", None) {
        text = out;
    }
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut queue: Vec<u8> = Vec::new();
    let mut newtext: Vec<u8> = Vec::with_capacity(text.len() + 16);
    // WordPress cuts the matched text off the front each time; the pattern has no lookbehind, so matching from an
    // offset is the same and copies nothing.
    let mut offset = 0;
    loop {
        let rest = &text[offset..];
        let Ok(Some(m)) = preg_match(&TAG, rest) else { break };
        let full = m.full().to_vec();
        let has_leading_slash = !m.get(1).unwrap_or_default().is_empty();
        let tag = m.get(2).unwrap_or_default().to_ascii_lowercase();
        let is_single = SINGLE_TAGS.contains(&&tag[..]);
        // PHP drops trailing unset groups from $regex: group 4 is set only by the attribute branch.
        let (mut pre_ws, mut attributes) = match m.get(4) {
            Some(ws) => (ws.to_vec(), php_trim(m.get(5).unwrap_or_default(), PHP_TRIM_DEFAULT).to_vec()),
            None => (Vec::new(), php_trim(m.get(3).unwrap_or_default(), PHP_TRIM_DEFAULT).to_vec()),
        };
        let has_self_closer = attributes.ends_with(b"/");
        newtext.append(&mut queue);
        let i = memmem::find(rest, &full).unwrap_or(0);
        let l = full.len();
        let mut out_tag: Vec<u8>;
        if has_leading_slash {
            out_tag = Vec::new();
            if stack.last() == Some(&tag) {
                out_tag = [&b"</"[..], &tag, b">"].concat();
                stack.pop();
            } else if let Some(j) = stack.iter().rposition(|t| *t == tag) {
                while stack.len() > j {
                    let closed = stack.pop().expect("the stack has j+1 tags");
                    queue.extend_from_slice(b"</");
                    queue.extend_from_slice(&closed);
                    queue.push(b'>');
                }
            }
        } else {
            if has_self_closer {
                if !is_single {
                    let trimmed = php_trim(&attributes[..attributes.len() - 1], PHP_TRIM_DEFAULT).to_vec();
                    attributes = [&trimmed[..], b"></", &tag].concat();
                }
            } else if is_single {
                pre_ws = b" ".to_vec();
                attributes.push(b'/');
            } else {
                if stack.last() == Some(&tag) && !NESTABLE_TAGS.contains(&&tag[..]) {
                    let closed = stack.pop().expect("a last tag");
                    queue = [&b"</"[..], &closed, b">"].concat();
                }
                stack.push(tag.clone());
            }
            if has_self_closer && is_single {
                pre_ws = b" ".to_vec();
            }
            out_tag = [&b"<"[..], &tag, &pre_ws, &attributes, b">"].concat();
            if !queue.is_empty() {
                queue.append(&mut out_tag);
            }
        }
        newtext.extend_from_slice(&rest[..i]);
        newtext.extend_from_slice(&out_tag);
        offset += i + l;
    }
    newtext.append(&mut queue);
    newtext.extend_from_slice(&text[offset..]);
    // `while ( $x = array_pop( $tagstack ) )` stops at a falsy name.
    while let Some(x) = stack.pop() {
        if x == b"0" {
            break;
        }
        newtext.extend_from_slice(b"</");
        newtext.extend_from_slice(&x);
        newtext.push(b'>');
    }
    let newtext = replace_one(&newtext, b"< !--", b"<!--").into_owned();
    Cow::Owned(replace_one(&newtext, b"<    !--", b"< !--").into_owned())
}
