//! The pure helpers behind `make_clickable()` and `links_add_base_url()`.

use std::sync::LazyLock;

use crate::preg::{Pattern, preg_match, preg_replace};
use crate::url::wp_parse_url;

/// WordPress's `_split_str_by_whitespace()`: cuts long text into chunks of about `goal` bytes, each ending at
/// whitespace where it can.
///
/// @param text
///     The text.
/// @param goal
///     The preferred chunk length.
/// @return
///     The chunks, which join back into `text`; a chunk is longer than `goal` only when it has no whitespace to cut
///     at.
pub fn split_str_by_whitespace(text: &[u8], goal: usize) -> Vec<&[u8]> {
    let space = |b: &u8| matches!(b, b'\r' | b'\n' | b'\t' | b'\x0B' | b'\x0C' | b' ');
    let mut chunks = Vec::new();
    let mut rest = text;
    while goal < rest.len() {
        let window = &rest[..goal + 1];
        let pos = match window.iter().rposition(space) {
            Some(p) => p,
            None => match rest[goal + 1..].iter().position(space) {
                Some(p) => goal + 1 + p,
                None => break,
            },
        };
        chunks.push(&rest[..pos + 1]);
        rest = &rest[pos + 1..];
    }
    // `if ( $text )`: an empty rest, or "0", is not pushed.
    if !rest.is_empty() && rest != b"0" {
        chunks.push(rest);
    }
    chunks
}

/// `WP_Http::make_absolute_url()`'s patterns.
static ABSOLUTE: LazyLock<(Pattern, Pattern)> =
    LazyLock::new(|| (Pattern::php("![^/]+/\\.\\./!"), Pattern::php("!^/(\\.\\./)+!")));

/// PHP's `empty()` on an optional string part.
fn filled(part: &Option<Vec<u8>>) -> Option<&[u8]> {
    part.as_deref().filter(|p| !p.is_empty() && *p != b"0")
}

/// `WP_Http::make_absolute_url( $maybe_relative_path, $url )` (`wp-includes/class-wp-http.php`).
///
/// @param relative
///     The URL that may be relative.
/// @param base
///     The URL to resolve it against.
/// @return
///     The absolute URL, or `relative` as it was when either URL can't be parsed or `relative` has a scheme.
pub fn make_absolute_url(relative: &[u8], base: &[u8]) -> Vec<u8> {
    if base.is_empty() || base == b"0" {
        return relative.to_vec();
    }
    let Some(base_parts) = wp_parse_url(base).filter(|p| *p != Default::default()) else {
        return relative.to_vec();
    };
    let Some(rel) = wp_parse_url(relative).filter(|p| *p != Default::default()) else {
        return relative.to_vec();
    };
    if filled(&rel.scheme).is_some() {
        return relative.to_vec();
    }
    let mut absolute = base_parts.scheme.clone().unwrap_or_default();
    absolute.extend_from_slice(b"://");
    let (host, port) = match &rel.host {
        Some(host) => (host.clone(), rel.port),
        None => (base_parts.host.clone().unwrap_or_default(), base_parts.port),
    };
    absolute.extend_from_slice(&host);
    if let Some(port) = port {
        absolute.extend_from_slice(format!(":{port}").as_bytes());
    }
    let mut path = filled(&base_parts.path).map_or_else(|| b"/".to_vec(), <[u8]>::to_vec);
    match filled(&rel.path) {
        Some(rel_path) if rel_path[0] == b'/' => path = rel_path.to_vec(),
        Some(rel_path) => {
            let keep = path.iter().rposition(|&b| b == b'/').map_or(0, |i| i + 1);
            path.truncate(keep);
            path.extend_from_slice(rel_path);
            let (segment, leading) = &*ABSOLUTE;
            // while ( strpos( $path, '../' ) > 1 ): found at an offset of 2 or more.
            while path.windows(3).position(|w| w == b"../").is_some_and(|at| at > 1) {
                match preg_replace(segment, &path, b"", None) {
                    Ok((out, count)) if count > 0 => path = out.into_owned(),
                    _ => break,
                }
            }
            if let Ok((out, _)) = preg_replace(leading, &path, b"", None) {
                path = out.into_owned();
            }
        }
        None => {}
    }
    if let Some(query) = filled(&rel.query) {
        path.push(b'?');
        path.extend_from_slice(query);
    }
    if let Some(fragment) = filled(&rel.fragment) {
        path.push(b'#');
        path.extend_from_slice(fragment);
    }
    let trimmed = &path[path.iter().take_while(|&&b| b == b'/').count()..];
    [absolute.as_slice(), b"/", trimmed].concat()
}

/// `is_serialized()`'s two patterns, by token.
static SERIALIZED: LazyLock<[Pattern; 3]> = LazyLock::new(|| {
    [Pattern::php("/^[saOE]:[0-9]+:/s"), Pattern::php("/^[bid]:[0-9.E+-]+;$/"), Pattern::php("/^[bid]:[0-9.E+-]+;/")]
});

/// WordPress's `is_serialized( $data )` (strict), for a string (`wp-includes/functions.php`).
///
/// @param data
///     The string.
/// @return
///     Whether it looks like PHP's `serialize()` output.
pub fn is_serialized(data: &[u8]) -> bool {
    let data = crate::strings::php_trim(data, crate::strings::PHP_TRIM_DEFAULT);
    if data == b"N;" {
        return true;
    }
    if data.len() < 4 || data[1] != b':' {
        return false;
    }
    let last = data[data.len() - 1];
    if last != b';' && last != b'}' {
        return false;
    }
    let matches = |p: &Pattern| matches!(preg_match(p, data), Ok(Some(_)));
    match data[0] {
        b's' => data[data.len() - 2] == b'"' && matches(&SERIALIZED[0]),
        b'a' | b'O' | b'E' => matches(&SERIALIZED[0]),
        b'b' | b'i' | b'd' => matches(&SERIALIZED[1]),
        _ => false,
    }
}

/// `preg_match( '|^<NAME[\s>]|i', $piece )`, as `make_clickable()` tests a piece.
///
/// @param piece
///     The piece.
/// @param name
///     The tag name, lowercase.
/// @return
///     Whether the piece opens that tag.
pub fn opens(piece: &[u8], name: &[u8]) -> bool {
    piece.len() > name.len() + 1
        && piece[0] == b'<'
        && piece[1..=name.len()].eq_ignore_ascii_case(name)
        && matches!(piece[name.len() + 1], b' ' | b'\t' | b'\n' | b'\x0B' | b'\x0C' | b'\r' | b'>')
}

/// `preg_match( '|^<\s*[\w]{1,20}+://|', $piece )`: the possessive count takes at most 20 word bytes.
///
/// @param piece
///     A piece starting with `<`.
/// @return
///     Whether a URL starts right inside it.
pub fn tag_starts_with_url(piece: &[u8]) -> bool {
    let after = &piece[1..];
    let spaces = after.iter().take_while(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\x0B' | b'\x0C' | b'\r')).count();
    let rest = &after[spaces..];
    let word = rest.iter().take(20).take_while(|b| b.is_ascii_alphanumeric() || **b == b'_').count();
    word > 0 && rest[word..].starts_with(b"://")
}

/// Whether any of `make_clickable()`'s three patterns could match: they need `://`, `www.`/`ftp.` (any case) or `@`.
///
/// @param piece
///     The piece.
/// @return
///     `false` only when none can match.
pub fn may_link(piece: &[u8]) -> bool {
    memchr::memchr(b'@', piece).is_some()
        || memchr::memmem::find(piece, b"://").is_some()
        || piece.windows(4).any(|w| w[3] == b'.' && (w[..3].eq_ignore_ascii_case(b"www") || w[..3].eq_ignore_ascii_case(b"ftp")))
}
