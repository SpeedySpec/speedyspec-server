//! Percent-encoding a decoded path for a `Location` header.

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

/// Everything that cannot appear raw in a URL path segment: controls, space, delimiters that would end or change the
/// path (`#`, `?`), `%` itself, and the characters RFC 3986 excludes. `/` is kept; non-ASCII is always encoded.
const PATH: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// Encodes an already-decoded path so it can be sent back to a client, for example in `Location`.
pub fn encode_path(path: &str) -> String {
    utf8_percent_encode(path, PATH).to_string()
}
