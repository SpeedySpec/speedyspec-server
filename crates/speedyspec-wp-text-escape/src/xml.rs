//! `esc_xml()`'s escaping, after its UTF-8 check and before its filter.

use memchr::memmem;

use crate::decode::QuoteStyle;
use crate::specialchars::wp_specialchars;

/// `esc_xml()`'s core: escapes the text the XML way (`_wp_specialchars( $piece, ENT_XML1 )`) and leaves CDATA
/// sections as they are.
///
/// WordPress splits with a regex: each `<![CDATA[` up to the first `]]>` after it is kept, and each run of text
/// before one (and whatever follows the last) is escaped on its own. An unterminated `<![CDATA[` is escaped.
///
/// One deliberate difference: WordPress's regex can exhaust PHP's PCRE backtrack limit (hundreds of unterminated
/// `<![CDATA[`, or about 1 MB of any text), and `esc_xml()` then returns `''`. This scan is linear and always
/// returns the escaped text (`compat/formatting/breaks.json`, `xml_cdata_backtrack`).
///
/// @param text
///     The text, already through `wp_check_invalid_utf8()`.
/// @param charset
///     `blog_charset`.
/// @return
///     The escaped text.
pub fn esc_xml_after_check(text: &[u8], charset: &str) -> Vec<u8> {
    let escape = |piece: &[u8], out: &mut Vec<u8>| {
        out.extend_from_slice(&wp_specialchars(piece, QuoteStyle::Xml, charset, false));
    };
    let mut out = Vec::with_capacity(text.len() + text.len() / 8);
    let mut rest = text;
    loop {
        let section = memmem::find(rest, b"<![CDATA[").and_then(|start| {
            let body = start + 9;
            memmem::find(&rest[body..], b"]]>").map(|end| (start, body + end + 3))
        });
        match section {
            Some((start, end)) => {
                escape(&rest[..start], &mut out);
                out.extend_from_slice(&rest[start..end]);
                rest = &rest[end..];
            }
            None => {
                escape(rest, &mut out);
                return out;
            }
        }
    }
}
