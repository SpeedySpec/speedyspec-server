//! WordPress's small formatting helpers, which fire no filters.

use std::borrow::Cow;
use std::sync::LazyLock;

use speedyspec_wp_utf8::scan_utf8;

use crate::preg::{Pattern, preg_match, preg_replace, preg_replace_callback};
use crate::strings::{StrReplace, replace_one, urldecode, urlencode};

/// WordPress's `zeroise()`: `sprintf( '%0' . $threshold . 's', $number )`.
///
/// @param number
///     The number, already a string.
/// @param threshold
///     The width; a negative one left-aligns, so the zeros go on the right, as PHP's `-` flag does.
/// @return
///     The padded string; never shorter than `number`.
pub fn zeroise(number: &[u8], threshold: i64) -> Vec<u8> {
    let width = usize::try_from(threshold.unsigned_abs()).unwrap_or(usize::MAX);
    let pad = width.saturating_sub(number.len());
    let mut out = Vec::with_capacity(number.len() + pad);
    if threshold >= 0 {
        out.resize(pad, b'0');
        out.extend_from_slice(number);
    } else {
        out.extend_from_slice(number);
        out.resize(number.len() + pad, b'0');
    }
    out
}

/// WordPress's `untrailingslashit()`: `rtrim( $value, '/\\' )`.
///
/// @param value
///     The path or URL.
/// @return
///     It without trailing `/` and `\`.
pub fn untrailingslashit(value: &[u8]) -> &[u8] {
    let end = value.iter().rposition(|&b| b != b'/' && b != b'\\').map_or(0, |i| i + 1);
    &value[..end]
}

/// WordPress's `trailingslashit()`.
///
/// @param value
///     The path or URL.
/// @return
///     It with exactly one trailing `/` (trailing `\` removed).
pub fn trailingslashit(value: &[u8]) -> Vec<u8> {
    [untrailingslashit(value), b"/"].concat()
}

/// PHP's `basename( $path, $suffix )` on `/` separators.
fn basename<'a>(path: &'a [u8], suffix: &[u8]) -> &'a [u8] {
    let end = path.iter().rposition(|&b| b != b'/').map_or(0, |i| i + 1);
    let trimmed = &path[..end];
    let base = &trimmed[trimmed.iter().rposition(|&b| b == b'/').map_or(0, |i| i + 1)..];
    match base.strip_suffix(suffix) {
        Some(stem) if !suffix.is_empty() && !stem.is_empty() => stem,
        _ => base,
    }
}

/// WordPress's `wp_basename()`: a `basename()` that splits on `/` and `\` and keeps multibyte names whole.
///
/// @param path
///     The path.
/// @param suffix
///     A suffix to remove from the name, if it ends with it.
/// @return
///     The last part of the path.
pub fn wp_basename(path: &[u8], suffix: &[u8]) -> Vec<u8> {
    let encoded = urlencode(path);
    let slashed = SLASHES.apply(&encoded);
    urldecode(basename(&slashed, suffix))
}

/// `wp_basename()`'s `str_replace( array( '%2F', '%5C' ), '/', ... )`.
static SLASHES: LazyLock<StrReplace> =
    LazyLock::new(|| StrReplace::new(vec![(b"%2F".to_vec(), b"/".to_vec()), (b"%5C".to_vec(), b"/".to_vec())]));

/// `wp_iso_descrambler()`'s two patterns.
static ISO: LazyLock<(Pattern, Pattern)> =
    LazyLock::new(|| (Pattern::php("#\\=\\?(.+)\\?Q\\?(.+)\\?\\=#i"), Pattern::php("#\\=([0-9a-f]{2})#i")));

/// WordPress's `wp_iso_descrambler()`: decodes an RFC 2047 `Q`-encoded header word.
///
/// @param subject
///     The header.
/// @return
///     The decoded text, or the header as it was when it isn't `=?charset?Q?...?=`.
pub fn wp_iso_descrambler(subject: &[u8]) -> Cow<'_, [u8]> {
    let (word, escape) = &*ISO;
    let Ok(Some(found)) = preg_match(word, subject) else {
        return Cow::Borrowed(subject);
    };
    let spaced = replace_one(found.get(2).unwrap_or_default(), b"_", b" ").into_owned();
    // _wp_iso_convert(): chr( hexdec( strtolower( $matches[1] ) ) ).
    let decoded = preg_replace_callback(escape, &spaced, |m| {
        let hex = m.get(1).unwrap_or_default();
        vec![u8::from_str_radix(std::str::from_utf8(hex).unwrap_or("0"), 16).unwrap_or(0)]
    });
    Cow::Owned(decoded.map_or(spaced.clone(), Cow::into_owned))
}

/// `wp_make_link_relative()`'s pattern.
static LINK_RELATIVE: LazyLock<Pattern> = LazyLock::new(|| Pattern::php("|^(https?:)?//[^/]+(/?.*)|i"));

/// WordPress's `wp_make_link_relative()`.
///
/// @param link
///     The URL.
/// @return
///     The URL without its scheme and host when it starts with `http://`, `https://` or `//`.
pub fn wp_make_link_relative(link: &[u8]) -> Cow<'_, [u8]> {
    preg_replace(&LINK_RELATIVE, link, b"$2", None).map_or(Cow::Borrowed(link), |(out, _)| out)
}

/// `url_shorten()`'s `str_replace( array( 'https://', 'http://', 'www.' ), '', $url )`.
static URL_PREFIXES: LazyLock<StrReplace> = LazyLock::new(|| {
    StrReplace::new(vec![(b"https://".to_vec(), Vec::new()), (b"http://".to_vec(), Vec::new()), (b"www.".to_vec(), Vec::new())])
});

/// WordPress's `url_shorten()`.
///
/// @param url
///     The URL.
/// @param length
///     The longest result before it is cut and given `&hellip;`.
/// @return
///     The URL without scheme, `www.` and trailing slashes, cut to `length - 3` bytes plus `&hellip;` when longer.
pub fn url_shorten(url: &[u8], length: i64) -> Vec<u8> {
    let stripped = URL_PREFIXES.apply(url);
    let short = untrailingslashit(&stripped);
    if i64::try_from(short.len()).unwrap_or(i64::MAX) > length {
        let keep = php_substr_len(short.len(), length - 3);
        return [&short[..keep], b"&hellip;"].concat();
    }
    short.to_vec()
}

/// How many bytes `substr( $s, 0, $length )` keeps of a string of `len` bytes.
fn php_substr_len(len: usize, length: i64) -> usize {
    if length >= 0 {
        usize::try_from(length).unwrap_or(usize::MAX).min(len)
    } else {
        len.saturating_sub(usize::try_from(length.unsigned_abs()).unwrap_or(usize::MAX))
    }
}

/// WordPress's `_deep_replace()`: removes every search string, again and again, until none is left.
///
/// @param search
///     The strings to remove, in order.
/// @param subject
///     The text.
/// @return
///     The text with no search string left in it.
pub fn deep_replace(search: &[&[u8]], subject: &[u8]) -> Vec<u8> {
    let mut current = subject.to_vec();
    loop {
        let mut changed = false;
        for needle in search {
            if let Cow::Owned(next) = replace_one(&current, needle, b"") {
                current = next;
                changed = true;
            }
        }
        if !changed {
            return current;
        }
    }
}

/// WordPress's `utf8_uri_encode()`: percent-encodes the non-ASCII bytes of UTF-8 text, lowercase hex.
///
/// @param text
///     The UTF-8 text.
/// @param length
///     The longest result in bytes, never cutting a character; 0 for no limit.
/// @param encode_ascii
///     Whether ASCII is `rawurlencode()`d too.
/// @return
///     The encoded text.
pub fn utf8_uri_encode(text: &[u8], length: usize, encode_ascii: bool) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(text.len() * 2);
    let mut values: Vec<u8> = Vec::with_capacity(4);
    let mut octets = 1;
    for &value in text {
        if value < 128 {
            let encoded = if encode_ascii { crate::strings::rawurlencode(&[value]) } else { vec![value] };
            if length != 0 && out.len() + encoded.len() > length {
                break;
            }
            out.extend_from_slice(&encoded);
        } else {
            if values.is_empty() {
                octets = match value {
                    ..=223 => 2,
                    224..=239 => 3,
                    _ => 4,
                };
            }
            values.push(value);
            if length != 0 && out.len() + octets * 3 > length {
                break;
            }
            if values.len() == octets {
                for &v in &values {
                    // PHP's dechex(): no leading zero, but every byte here is >= 0x80.
                    out.extend_from_slice(&[b'%', HEX[usize::from(v >> 4)], HEX[usize::from(v & 15)]]);
                }
                values.clear();
                octets = 1;
            }
        }
    }
    out
}

/// WordPress's `wp_truncate_slug()` (7.2): cuts a slug to `length` bytes without splitting a percent-encoded
/// character, then removes trailing `-`, whether or not it was cut.
///
/// A slug longer than `length` that `urldecode()` leaves as it is is cut at `length` bytes. One that it changes
/// (a `%XX` sequence, or a `+`, which decodes to a space) is re-encoded by `utf8_uri_encode( $decoded, $length,
/// true )`: ASCII `rawurlencode`d (uppercase hex), multibyte characters in lowercase hex, whole characters only.
///
/// @param slug
///     The slug.
/// @param length
///     The longest slug in bytes; WordPress's default is 200.
/// @return
///     The truncated slug.
pub fn wp_truncate_slug(slug: &[u8], length: usize) -> Vec<u8> {
    let mut slug = slug.to_vec();
    if slug.len() > length {
        let decoded = urldecode(&slug);
        if decoded == slug {
            slug.truncate(length);
        } else {
            slug = utf8_uri_encode(&decoded, length, true);
        }
    }
    let end = slug.iter().rposition(|&b| b != b'-').map_or(0, |i| i + 1);
    slug.truncate(end);
    slug
}

/// WordPress's `antispambot()`: writes each character of an address as itself, an HTML numeric reference or
/// percent-encoded bytes, chosen at random.
///
/// @param email
///     The address.
/// @param hex_encoding
///     1 to allow the percent-encoded form.
/// @param rand
///     PHP's `rand( $min, $max )`.
/// @return
///     The obfuscated address, `@` always as `&#64;`; invalid bytes are copied as they are.
pub fn antispambot_core(email: &[u8], hex_encoding: i64, rand: &mut dyn FnMut(i64, i64) -> i64) -> Vec<u8> {
    let mut out = Vec::with_capacity(email.len() * 4);
    let mut at = 0;
    while at < email.len() {
        let was_at = at;
        let scan = scan_utf8(email, at, None, Some(1));
        at = scan.at;
        if scan.code_points == 0 && scan.invalid_length == 0 {
            break;
        }
        let character = &email[was_at..at];
        if !character.is_empty() {
            match rand(0, 1 + hex_encoding) {
                0 => {
                    let code_point = crate::strings::mb_ord(character).unwrap_or(0);
                    out.extend_from_slice(format!("&#{code_point};").as_bytes());
                }
                1 => out.extend_from_slice(character),
                2 => {
                    for b in character {
                        out.extend_from_slice(format!("%{b:02x}").as_bytes());
                    }
                }
                _ => {}
            }
        }
        out.extend_from_slice(&email[at..at + scan.invalid_length]);
        at += scan.invalid_length;
    }
    replace_one(&out, b"@", b"&#64;").into_owned()
}
