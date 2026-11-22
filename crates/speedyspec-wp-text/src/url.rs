//! PHP's `parse_url()` and WordPress's `wp_parse_url()`.
//!
//! `esc_url()`, `_make_clickable_rel_attr()`, the link context and `WP_Http::make_absolute_url()` all read URLs
//! through these. They live here until the HTTP seam ports `wp_parse_url()`; then this module defers to it.

use memchr::{memchr, memrchr};

/// The parts of a URL, as `parse_url()` returns them.
///
/// The parts are owned: PHP replaces every control character in them with `_`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UrlParts {
    pub scheme: Option<Vec<u8>>,
    pub host: Option<Vec<u8>>,
    pub port: Option<u16>,
    pub user: Option<Vec<u8>>,
    pub pass: Option<Vec<u8>>,
    pub path: Option<Vec<u8>>,
    pub query: Option<Vec<u8>>,
    pub fragment: Option<Vec<u8>>,
}

/// A component with PHP's `php_replace_controlchars()` applied (`iscntrl()` in the C locale).
fn part(bytes: &[u8]) -> Option<Vec<u8>> {
    Some(bytes.iter().map(|&b| if b < 0x20 || b == 0x7F { b'_' } else { b }).collect())
}

/// PHP's `binary_strcspn()`: the offset of the first of `chars` in `url[from..to]`, or `to`.
fn strcspn(url: &[u8], from: usize, to: usize, chars: &[u8]) -> usize {
    url[from..to].iter().position(|b| chars.contains(b)).map_or(to, |i| from + i)
}

/// `ZEND_STRTOL( buf, &end, 10 )` on a short buffer: `None` when no digit was read.
fn strtol(buf: &[u8]) -> Option<i64> {
    let mut i = 0;
    while i < buf.len() && matches!(buf[i], b' ' | b'\t' | b'\n' | b'\x0B' | b'\x0C' | b'\r') {
        i += 1;
    }
    let negative = i < buf.len() && buf[i] == b'-';
    if i < buf.len() && matches!(buf[i], b'+' | b'-') {
        i += 1;
    }
    let digits = buf[i..].iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let value: i64 = buf[i..i + digits].iter().fold(0, |n, d| n * 10 + i64::from(d - b'0'));
    Some(if negative { -value } else { value })
}

/// Whether `url[at..]` starts with `//`.
fn double_slash(url: &[u8], at: usize) -> bool {
    at + 1 < url.len() && url[at] == b'/' && url[at + 1] == b'/'
}

/// Where `php_url_parse_ex2()` jumps next.
enum Next {
    Port(usize),
    Host,
    Path,
}

/// PHP 8.5's `parse_url()` (`php_url_parse_ex2()` in `ext/standard/url.c`), transcribed.
///
/// @param url
///     The URL.
/// @return
///     Its parts, or `None` where PHP returns `false`.
pub fn parse_url(url: &[u8]) -> Option<UrlParts> {
    let ue = url.len();
    let mut ret = UrlParts::default();
    let mut port: u16 = 0;
    let mut has_port = false;
    let mut s = 0;

    let colon = memchr(b':', url);
    let mut next = match colon {
        Some(e) if e != s => {
            let valid = url[s..e].iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'.' | b'-'));
            if !valid {
                if e + 1 < ue && e < strcspn(url, s, ue, b"?#") {
                    Next::Port(e)
                } else if double_slash(url, s) {
                    s += 2;
                    Next::Host
                } else {
                    Next::Path
                }
            } else if e + 1 == ue {
                ret.scheme = part(&url[s..e]);
                return Some(ret);
            } else if url[e + 1] != b'/' {
                let mut p = e + 1;
                while p < ue && url[p].is_ascii_digit() {
                    p += 1;
                }
                if (p == ue || url[p] == b'/') && p - e < 7 {
                    Next::Port(e)
                } else {
                    ret.scheme = part(&url[s..e]);
                    s = e + 1;
                    Next::Path
                }
            } else {
                ret.scheme = part(&url[s..e]);
                if e + 2 < ue && url[e + 2] == b'/' {
                    s = e + 3;
                    if url[..e].eq_ignore_ascii_case(b"file") && e + 3 < ue && url[e + 3] == b'/' {
                        if e + 5 < ue && url[e + 5] == b':' {
                            s = e + 4;
                        }
                        Next::Path
                    } else {
                        Next::Host
                    }
                } else {
                    s = e + 1;
                    Next::Path
                }
            }
        }
        Some(e) => Next::Port(e),
        None if double_slash(url, s) => {
            s += 2;
            Next::Host
        }
        None => Next::Path,
    };

    if let Next::Port(e) = next {
        let p = e + 1;
        let mut pp = p;
        while pp < ue && pp - p < 6 && url[pp].is_ascii_digit() {
            pp += 1;
        }
        next = if pp - p > 0 && pp - p < 6 && (pp == ue || url[pp] == b'/') {
            match strtol(&url[p..pp]) {
                Some(n) if (0..=65535).contains(&n) => {
                    has_port = true;
                    port = n as u16;
                    if double_slash(url, s) {
                        s += 2;
                    }
                }
                _ => return None,
            }
            Next::Host
        } else if p == pp && pp == ue {
            return None;
        } else if double_slash(url, s) {
            s += 2;
            Next::Host
        } else {
            Next::Path
        };
    }

    if let Next::Host = next {
        let e = strcspn(url, s, ue, b"/?#");
        if let Some(at) = memrchr(b'@', &url[s..e]).map(|i| s + i) {
            match memchr(b':', &url[s..at]).map(|i| s + i) {
                Some(c) => {
                    ret.user = part(&url[s..c]);
                    ret.pass = part(&url[c + 1..at]);
                }
                None => ret.user = part(&url[s..at]),
            }
            s = at + 1;
        }
        let colon = if s < ue && url[s] == b'[' && e > 0 && url[e - 1] == b']' {
            None
        } else {
            memrchr(b':', &url[s..e]).map(|i| s + i)
        };
        let p = match colon {
            Some(c) => {
                if port == 0 {
                    let q = c + 1;
                    if e - q > 5 {
                        return None;
                    } else if e > q {
                        match strtol(&url[q..e]) {
                            Some(n) if (0..=65535).contains(&n) => {
                                has_port = true;
                                port = n as u16;
                            }
                            _ => return None,
                        }
                    }
                }
                c
            }
            None => e,
        };
        if p <= s {
            return None;
        }
        ret.host = part(&url[s..p]);
        ret.port = has_port.then_some(port);
        if e == ue {
            return Some(ret);
        }
        s = e;
    }

    let mut e = ue;
    if let Some(h) = memchr(b'#', &url[s..e]).map(|i| s + i) {
        ret.fragment = part(&url[h + 1..e]);
        e = h;
    }
    if let Some(q) = memchr(b'?', &url[s..e]).map(|i| s + i) {
        ret.query = part(&url[q + 1..e]);
        e = q;
    }
    if s < e || s == ue {
        ret.path = part(&url[s..e]);
    }
    ret.port = has_port.then_some(port);
    Some(ret)
}

/// WordPress's `wp_parse_url( $url )` (`wp-includes/http.php`): `parse_url()` that also reads URLs starting with
/// `//` or `/`, by parsing them behind a placeholder scheme and host and dropping those again.
///
/// @param url
///     The URL, already a string.
/// @return
///     Its parts, or `None` where WordPress returns `false`.
pub fn wp_parse_url(url: &[u8]) -> Option<UrlParts> {
    if url.starts_with(b"//") {
        let mut parts = parse_url(&[&b"placeholder:"[..], url].concat())?;
        parts.scheme = None;
        Some(parts)
    } else if url.starts_with(b"/") {
        let mut parts = parse_url(&[&b"placeholder://placeholder"[..], url].concat())?;
        parts.scheme = None;
        parts.host = None;
        Some(parts)
    } else {
        parse_url(url)
    }
}
