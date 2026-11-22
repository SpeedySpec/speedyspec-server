//! `esc_url()`'s core: everything before its `clean_url` filter.

use std::borrow::Cow;
use std::rc::Rc;

use speedyspec_wp_kses_contracts::{EntityContext, EntityNames, Protocols};
use speedyspec_wp_kses_lexical::{bad_protocol, normalize_entities};

use crate::helpers::deep_replace;
use crate::strings::{PHP_TRIM_DEFAULT, replace_one};
use crate::url::wp_parse_url;

/// `[^a-z0-9-~+_.?#=!&;,/:%@$\|*'()\[\]\x80-\xff]` with `/i`, as a table of the bytes `esc_url()` keeps.
static KEEP: [bool; 256] = {
    let mut table = [false; 256];
    let mut b = 0;
    while b < 256 {
        let c = b as u8;
        table[b] = c.is_ascii_alphanumeric() || c >= 0x80;
        b += 1;
    }
    let punctuation = b"-~+_.?#=!&;,/:%@$|*'()[]";
    let mut i = 0;
    while i < punctuation.len() {
        table[punctuation[i] as usize] = true;
        i += 1;
    }
    table
};

/// `esc_url()`'s `preg_replace( '|[^a-z0-9-~+_.?#=!&;,/:%@$\|*\'()\[\]\\x80-\\xff]|i', '', $url )`.
///
/// @param url
///     The URL.
/// @return
///     The URL without the bytes `esc_url()` drops; borrowed when it drops none.
pub fn esc_url_characters(url: &[u8]) -> Cow<'_, [u8]> {
    match url.iter().position(|&b| !KEEP[usize::from(b)]) {
        None => Cow::Borrowed(url),
        Some(first) => {
            let mut out = url[..first].to_vec();
            out.extend(url[first..].iter().copied().filter(|&b| KEEP[usize::from(b)]));
            Cow::Owned(out)
        }
    }
}

/// `esc_url()` before its `clean_url` filter.
///
/// @param url
///     The URL; `""` returns `""` and the caller fires no filter.
/// @param given
///     The protocols the caller passed as an array; an `https` first also makes a bare URL `https://`.
/// @param allowed
///     `wp_allowed_protocols()`, asked only when the caller passed none and the URL needs checking, as WordPress
///     asks it.
/// @param display
///     The `display` context: entities are normalized and `&amp;` and `'` written as `&#038;` and `&#039;`.
/// @param names
///     kses's HTML and XML entity names (`$allowedentitynames`, `$allowedxmlentitynames`).
/// @return
///     The cleaned URL, or `""` when it uses a protocol not kept; an error only from `allowed`.
pub fn esc_url_core<E>(
    url: &[u8],
    given: Option<&Protocols>,
    allowed: impl FnOnce() -> Result<Rc<Protocols>, E>,
    display: bool,
    names: (&EntityNames, &EntityNames),
) -> Result<Vec<u8>, E> {
    if url.is_empty() {
        return Ok(Vec::new());
    }
    let start = url.iter().position(|b| !PHP_TRIM_DEFAULT.contains(b)).unwrap_or(url.len());
    let spaced = replace_one(&url[start..], b" ", b"%20");
    let mut url = esc_url_characters(&spaced).into_owned();
    if url.is_empty() {
        return Ok(url);
    }
    if !url.get(..7).is_some_and(|head| head.eq_ignore_ascii_case(b"mailto:")) {
        url = deep_replace(&[b"%0d", b"%0a", b"%0D", b"%0A"], &url);
    }
    url = replace_one(&url, b";//", b"://").into_owned();
    // `deep_replace` can empty the URL, and PHP's `$url[0]` on "" is "" (with a warning).
    let first = url.first().copied();
    if !url.contains(&b':') && !matches!(first, Some(b'/' | b'#' | b'?')) && !looks_like_php_script(&url) {
        let https = given.is_some_and(|p| p.0.first().is_some_and(|f| &**f == b"https"));
        url = [if https { &b"https://"[..] } else { b"http://" }, &url].concat();
    }
    if display {
        url = normalize_entities(&url, EntityContext::Html, names.0, names.1).into_owned();
        url = replace_one(&url, b"&amp;", b"&#038;").into_owned();
        url = replace_one(&url, b"'", b"&#039;").into_owned();
    }
    if url.contains(&b'[') || url.contains(&b']') {
        url = escape_brackets(url);
    }
    if url.first() == Some(&b'/') {
        return Ok(url);
    }
    let fetched;
    let list = match given {
        Some(p) => p,
        None => {
            fetched = allowed()?;
            &*fetched
        }
    };
    let good = bad_protocol(&url, list);
    if good.to_ascii_lowercase() != url.to_ascii_lowercase() {
        return Ok(Vec::new());
    }
    Ok(good.into_owned())
}

/// `esc_url()`'s `preg_match( '/^[a-z0-9-]+?\.php/i', $url )`: a bare `name.php` gets no scheme added.
///
/// @param url
///     The URL.
/// @return
///     Whether it starts with letters, digits or `-` and then `.php`.
pub fn looks_like_php_script(url: &[u8]) -> bool {
    let name = url.iter().take_while(|b| b.is_ascii_alphanumeric() || **b == b'-').count();
    name > 0 && url[name..].get(..4).is_some_and(|ext| ext.eq_ignore_ascii_case(b".php"))
}

/// `esc_url()`'s IPv6 handling: brackets after the scheme, credentials, host and port become `%5B` and `%5D`.
fn escape_brackets(url: Vec<u8>) -> Vec<u8> {
    let parsed = wp_parse_url(&url);
    let mut front = Vec::new();
    match parsed.as_ref().and_then(|p| p.scheme.as_ref()) {
        Some(scheme) => {
            front.extend_from_slice(scheme);
            front.extend_from_slice(b"://");
        }
        None if url.first() == Some(&b'/') => front.extend_from_slice(b"//"),
        None => {}
    }
    if let Some(p) = &parsed {
        if let Some(user) = &p.user {
            front.extend_from_slice(user);
        }
        if let Some(pass) = &p.pass {
            front.push(b':');
            front.extend_from_slice(pass);
        }
        if p.user.is_some() || p.pass.is_some() {
            front.push(b'@');
        }
        if let Some(host) = &p.host {
            front.extend_from_slice(host);
        }
        if let Some(port) = p.port {
            front.push(b':');
            front.extend_from_slice(port.to_string().as_bytes());
        }
    }
    let end_dirty = replace_one(&url, &front, b"").into_owned();
    let end_clean = replace_one(&replace_one(&end_dirty, b"[", b"%5B"), b"]", b"%5D").into_owned();
    replace_one(&url, &end_dirty, &end_clean).into_owned()
}
