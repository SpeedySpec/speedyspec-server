//! `TextApi`'s link filters: `make_clickable()`, the rel callbacks and `links_add_*()`.
//!
//! Each is a transcription with WordPress's patterns. Their callbacks call the filtered escapers, so they live on
//! the facade; PHP's `$_links_add_*` globals become arguments.

use std::cell::RefCell;
use std::sync::LazyLock;

use speedyspec_hooks::Value as HookValue;
use speedyspec_wp_kses_lexical::hair;
use speedyspec_wp_text_contracts::TextError;

use crate::api::TextApi;
use crate::api_sanitize::kses_error;
use crate::links::{is_serialized, make_absolute_url, split_str_by_whitespace};
use crate::preg::{Captures, Pattern, SplitFlags, preg_match, preg_match_all_group, preg_replace, preg_replace_callback, preg_split};
use crate::strings::{bytes_value, php_trim, to_php_string, PHP_TRIM_DEFAULT};
use crate::url::wp_parse_url;

/// `make_clickable()`'s URL pattern, copied from 7.1.5 with PHP's string escapes resolved.
const URL_CLICKABLE: &str = r"~
				([\s(<.,;:!?])                                # 1: Leading whitespace, or punctuation.
				(                                              # 2: URL.
					[\w]{1,20}+://                                # Scheme and hier-part prefix.
					(?=\S{1,2000}\s)                               # Limit to URLs less than about 2000 characters long.
					[\w\x80-\xff#%\~/@\[\]*(+=&$-]*+         # Non-punctuation URL character.
					(?:                                            # Unroll the Loop: Only allow punctuation URL character if followed by a non-punctuation URL character.
						['.,;:!?)]                                    # Punctuation URL character.
						[\w\x80-\xff#%\~/@\[\]*(+=&$-]++         # Non-punctuation URL character.
					)*
				)
				(\)?)                                          # 3: Trailing closing parenthesis (for parenthesis balancing post processing).
				(\.\w{2,6})?                                 # 4: Allowing file extensions (e.g., .jpg, .png).
			~xS";

struct LinkPatterns {
    split_tags: Pattern,
    code: Pattern,
    pre: Pattern,
    script: Pattern,
    style: Pattern,
    tag_url: Pattern,
    url: Pattern,
    web_ftp: Pattern,
    email: Pattern,
    nested_links: Pattern,
    anchor: Pattern,
    script_style: Pattern,
    targeted_anchor: Pattern,
    unescaped_quote: Pattern,
    escaped_quote: Pattern,
    whitespace: Pattern,
    protocol: Pattern,
    target_attr: Pattern,
}

static LINKS: LazyLock<LinkPatterns> = LazyLock::new(|| LinkPatterns {
    split_tags: Pattern::php("/(<[^<>]+>)/"),
    code: Pattern::php("|^<code[\\s>]|i"),
    pre: Pattern::php("|^<pre[\\s>]|i"),
    script: Pattern::php("|^<script[\\s>]|i"),
    style: Pattern::php("|^<style[\\s>]|i"),
    tag_url: Pattern::php("|^<\\s*[\\w]{1,20}+://|"),
    url: Pattern::php(URL_CLICKABLE),
    web_ftp: Pattern::php("#([\\s>])((www|ftp)\\.[\\w\\x80-\\xff\\#$%&~/.\\-;:=,?@\\[\\]+]+)#is"),
    email: Pattern::php("#([\\s>])([.0-9a-z_+-]+)@(([0-9a-z-]+\\.)+[0-9a-z]{2,})#i"),
    nested_links: Pattern::php("#(<a([ \\r\\n\\t]+[^>]+?>|>))<a [^>]+?>([^>]+?)</a></a>#i"),
    anchor: Pattern::php("|<a (.+?)>|i"),
    script_style: Pattern::php("/<(script|style).*?<\\/\\1>/si"),
    targeted_anchor: Pattern::php("|<a\\s([^>]*target\\s*=[^>]*)>|i"),
    unescaped_quote: Pattern::php("/(^|[^\\\\])['\"]/"),
    escaped_quote: Pattern::php("/\\\\(['\"])/"),
    whitespace: Pattern::php("/\\s/"),
    protocol: Pattern::php("#^(\\w{1,20}):#"),
    target_attr: Pattern::php("|( target=(['\"])(.*?)\\2)|i"),
});

/// Collects the first error a callback hits, since PCRE callbacks can't return one.
struct Failure(RefCell<Option<TextError>>);

impl Failure {
    fn new() -> Failure {
        Failure(RefCell::new(None))
    }

    /// The callback's result, or `fallback` after recording its error.
    fn keep<T>(&self, result: Result<T, TextError>, fallback: T) -> T {
        result.unwrap_or_else(|e| {
            self.0.borrow_mut().get_or_insert(e);
            fallback
        })
    }

    fn finish<T>(self, value: T) -> Result<T, TextError> {
        match self.0.into_inner() {
            Some(e) => Err(e),
            None => Ok(value),
        }
    }
}

/// A PCRE replacement's result; PHP's `null` on failure is read as `""` by what follows.
fn replaced(result: Result<std::borrow::Cow<'_, [u8]>, crate::preg::PregError>) -> Vec<u8> {
    result.map_or_else(|_| Vec::new(), std::borrow::Cow::into_owned)
}

/// PHP's truthiness of a string.
fn truthy(text: &[u8]) -> bool {
    !(text.is_empty() || text == b"0")
}

/// `array_unique()` on strings: the first of each.
fn unique(items: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::with_capacity(items.len());
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

impl TextApi {
    /// A filtered escaper's result as a string.
    fn escaped(&self, value: Result<HookValue, TextError>) -> Result<Vec<u8>, TextError> {
        Ok(to_php_string(&value?).unwrap_or_default())
    }

    /// `esc_attr()` as a string.
    fn attr(&self, text: &[u8]) -> Result<Vec<u8>, TextError> {
        self.escaped(self.d.escape.esc_attr(text))
    }

    /// `esc_url()` as a string.
    fn url(&self, url: &[u8]) -> Result<Vec<u8>, TextError> {
        self.escaped(self.esc_url(url, None, b"display"))
    }

    /// `_make_clickable_rel_attr( $url )`.
    fn clickable_rel_attr(&self, url: &[u8]) -> Result<Vec<u8>, TextError> {
        let scheme = wp_parse_url(url).and_then(|p| p.scheme).unwrap_or_default().to_ascii_lowercase();
        let protocols = self.d.kses.allowed_protocols().map_err(kses_error)?;
        let internal = self.d.link_context.is_internal_link(url)?;
        let nofollow = protocols.0.iter().any(|p| (&**p == b"https" || &**p == b"http") && **p == *scheme);
        let mut parts: Vec<&[u8]> = Vec::new();
        if !internal && nofollow {
            parts.push(b"nofollow");
        }
        if self.d.hooks.current_filter() == Some(HookValue::Str("comment_text".into())) {
            parts.push(b"ugc");
        }
        let rel = self.filter(&self.hook("make_clickable_rel"), bytes_value(parts.join(&b' ')), || {
            vec![bytes_value(url.to_vec())]
        })?;
        let rel = to_php_string(&rel).unwrap_or_default();
        Ok(if truthy(&rel) { [&b" rel=\""[..], &self.attr(&rel)?, b"\""].concat() } else { Vec::new() })
    }

    /// `_make_url_clickable_cb( $matches )`.
    fn url_clickable_cb(&self, m: &Captures<'_>) -> Result<Vec<u8>, TextError> {
        let whole = m.get(0).unwrap_or_default();
        let mut url = m.get(2).unwrap_or_default().to_vec();
        let close = m.get(3).unwrap_or_default();
        let mut suffix = Vec::new();
        // strpos( $url, '(' ) is truthy only past offset 0.
        if close == b")" && url.iter().position(|&b| b == b'(').is_some_and(|at| at > 0) {
            url.extend_from_slice(close);
        } else {
            suffix.extend_from_slice(close);
        }
        if let Some(ext) = m.get(4).filter(|e| truthy(e)) {
            url.extend_from_slice(ext);
        }
        let count = |u: &[u8], c: u8| u.iter().filter(|&&b| b == c).count();
        while count(&url, b'(') < count(&url, b')') {
            let at = url.iter().rposition(|&b| b == b')').expect("counted one");
            suffix = [&url[at..], suffix.as_slice()].concat();
            url.truncate(at);
        }
        let url = self.url(&url)?;
        if !truthy(&url) {
            return Ok(whole.to_vec());
        }
        let rel = self.clickable_rel_attr(&url)?;
        Ok([m.get(1).unwrap_or_default(), b"<a href=\"", &url, b"\"", &rel, b">", &url, b"</a>", &suffix].concat())
    }

    /// `_make_web_ftp_clickable_cb( $matches )`.
    fn web_ftp_clickable_cb(&self, m: &Captures<'_>) -> Result<Vec<u8>, TextError> {
        let mut dest = [&b"http://"[..], m.get(2).unwrap_or_default()].concat();
        let mut ret = Vec::new();
        if let Some(&last) = dest.last().filter(|b| matches!(b, b'.' | b',' | b';' | b':' | b')')) {
            ret.push(last);
            dest.pop();
        }
        let dest = self.url(&dest)?;
        if !truthy(&dest) {
            return Ok(m.get(0).unwrap_or_default().to_vec());
        }
        let rel = self.clickable_rel_attr(&dest)?;
        Ok([m.get(1).unwrap_or_default(), b"<a href=\"", &dest, b"\"", &rel, b">", &dest, b"</a>", &ret].concat())
    }

    /// `make_clickable( $text )`: plain URLs, `www.`/`ftp.` hosts and email addresses become links, outside tags
    /// and outside `code`, `pre`, `script` and `style`.
    ///
    /// @param text
    ///     The text.
    /// @return
    ///     The text with links.
    pub fn make_clickable(&self, text: &[u8]) -> Result<Vec<u8>, TextError> {
        let p = &*LINKS;
        let flags = SplitFlags { delim_capture: true, no_empty: false };
        let pieces = preg_split(&p.split_tags, text, -1, flags).unwrap_or_default();
        let mut r = Vec::with_capacity(text.len() + text.len() / 4);
        let mut nested: i64 = 0;
        let starts = |pattern: &Pattern, piece: &[u8]| matches!(preg_match(pattern, piece), Ok(Some(_)));
        for piece in pieces {
            if starts(&p.code, piece) || starts(&p.pre, piece) || starts(&p.script, piece) || starts(&p.style, piece) {
                nested += 1;
            } else if nested != 0
                && matches!(piece.to_ascii_lowercase().as_slice(), b"</code>" | b"</pre>" | b"</script>" | b"</style>")
            {
                nested -= 1;
            }
            if nested != 0 || !truthy(piece) || (piece[0] == b'<' && !starts(&p.tag_url, piece)) {
                r.extend_from_slice(piece);
                continue;
            }
            if piece.len() > 10000 {
                for chunk in split_str_by_whitespace(piece, 2100) {
                    if chunk.len() > 2101 {
                        r.extend_from_slice(chunk);
                    } else {
                        r.extend_from_slice(&self.make_clickable(chunk)?);
                    }
                }
                continue;
            }
            let padded = [b" ", piece, b" "].concat();
            let failure = Failure::new();
            let ret = replaced(preg_replace_callback(&p.url, &padded, |m| failure.keep(self.url_clickable_cb(m), Vec::new())));
            let ret = replaced(preg_replace_callback(&p.web_ftp, &ret, |m| failure.keep(self.web_ftp_clickable_cb(m), Vec::new())));
            let ret = replaced(preg_replace_callback(&p.email, &ret, |m| {
                let email = [m.get(2).unwrap_or_default(), b"@", m.get(3).unwrap_or_default()].concat();
                [m.get(1).unwrap_or_default(), b"<a href=\"mailto:", &email, b"\">", &email, b"</a>"].concat()
            }));
            failure.finish(())?;
            // substr( $ret, 1, -1 ).
            if ret.len() > 2 {
                r.extend_from_slice(&ret[1..ret.len() - 1]);
            }
        }
        Ok(replaced(preg_replace(&p.nested_links, &r, b"$1$3</a>", None).map(|(out, _)| out)))
    }

    /// `wp_rel_callback( $matches, $rel )`: one `<a ...>`'s attributes with `rel` merged in.
    ///
    /// @param inner
    ///     What `<a (.+?)>` captured.
    /// @param rel
    ///     The rel values to add; `nofollow` is dropped for an internal `href`.
    /// @return
    ///     The rebuilt `<a ...>`.
    pub fn wp_rel_callback(&self, inner: &[u8], rel: &[u8]) -> Result<Vec<u8>, TextError> {
        let protocols = self.d.kses.allowed_protocols().map_err(kses_error)?;
        let uris = self.d.kses.uri_attributes().map_err(kses_error)?;
        let mut atts = hair(inner, &protocols, &uris);
        let mut rel = rel.to_vec();
        if let Some(href) = atts.iter().find(|a| &*a.name == b"href")
            && self.d.link_context.is_internal_link(&href.value)?
        {
            rel = php_trim(&crate::strings::replace_one(&rel, b"nofollow", b""), PHP_TRIM_DEFAULT).to_vec();
        }
        let mut text = inner.to_vec();
        if let Some(at) = atts.iter().position(|a| &*a.name == b"rel") {
            let trim_all = |s: &[u8]| -> Vec<Vec<u8>> {
                s.split(|&b| b == b' ').map(|p| php_trim(p, PHP_TRIM_DEFAULT).to_vec()).collect()
            };
            let mut parts = trim_all(&atts[at].value);
            parts.extend(trim_all(&rel));
            rel = unique(parts).join(&b' ');
            atts.remove(at);
            let mut html = Vec::new();
            for a in &atts {
                if a.vless {
                    html.extend_from_slice(&a.name);
                    html.push(b' ');
                } else {
                    html.extend_from_slice(&a.name);
                    html.extend_from_slice(b"=\"");
                    html.extend_from_slice(&self.attr(&a.value)?);
                    html.extend_from_slice(b"\" ");
                }
            }
            text = php_trim(&html, PHP_TRIM_DEFAULT).to_vec();
        }
        let rel_attr = if truthy(&rel) { [&b" rel=\""[..], &self.attr(&rel)?, b"\""].concat() } else { Vec::new() };
        Ok([&b"<a "[..], &text, &rel_attr, b">"].concat())
    }

    /// `wp_rel_nofollow_callback( $matches )`.
    ///
    /// @param inner
    ///     What `<a (.+?)>` captured.
    /// @return
    ///     The rebuilt `<a ...>` with `nofollow`.
    pub fn wp_rel_nofollow_callback(&self, inner: &[u8]) -> Result<Vec<u8>, TextError> {
        self.wp_rel_callback(inner, b"nofollow")
    }

    /// `wp_rel_nofollow()` and `wp_rel_ugc()`: unslash, rebuild every `<a ...>`, slash again.
    fn rel_links(&self, text: &[u8], rel: &[u8]) -> Result<Vec<u8>, TextError> {
        let unslashed = speedyspec_wp_text_escape::stripslashes(text);
        let failure = Failure::new();
        let out = replaced(preg_replace_callback(&LINKS.anchor, &unslashed, |m| {
            failure.keep(self.wp_rel_callback(m.get(1).unwrap_or_default(), rel), Vec::new())
        }));
        failure.finish(speedyspec_wp_text_escape::addslashes(&out).into_owned())
    }

    /// `wp_rel_nofollow( $text )`.
    ///
    /// @param text
    ///     Slashed HTML.
    /// @return
    ///     Slashed HTML with `rel="nofollow"` on every external link.
    pub fn wp_rel_nofollow(&self, text: &[u8]) -> Result<Vec<u8>, TextError> {
        self.rel_links(text, b"nofollow")
    }

    /// `wp_rel_ugc( $text )`.
    ///
    /// @param text
    ///     Slashed HTML.
    /// @return
    ///     Slashed HTML with `rel="nofollow ugc"` (`ugc` alone on internal links).
    pub fn wp_rel_ugc(&self, text: &[u8]) -> Result<Vec<u8>, TextError> {
        self.rel_links(text, b"nofollow ugc")
    }

    /// `wp_targeted_link_rel_callback( $matches )` (deprecated in 6.7.0).
    ///
    /// @param link_html
    ///     What `<a\s([^>]*target\s*=[^>]*)>` captured.
    /// @return
    ///     The `<a ...>` with the filtered rel (default `noopener`) added when it has a `target`.
    pub fn wp_targeted_link_rel_callback(&self, link_html: &[u8]) -> Result<Vec<u8>, TextError> {
        let p = &*LINKS;
        self.d.diagnostics.deprecated_function("wp_targeted_link_rel_callback", "6.7.0", "")?;
        let original = link_html;
        let escaped = !matches!(preg_match(&p.unescaped_quote, link_html), Ok(Some(_)));
        let link_html = if escaped {
            replaced(preg_replace(&p.escaped_quote, link_html, b"$1", None).map(|(o, _)| o))
        } else {
            link_html.to_vec()
        };
        let protocols = self.d.kses.allowed_protocols().map_err(kses_error)?;
        let uris = self.d.kses.uri_attributes().map_err(kses_error)?;
        let atts = hair(&link_html, &protocols, &uris);
        let rel = self.filter(&self.hook("wp_targeted_link_rel"), HookValue::Str("noopener".into()), || {
            vec![bytes_value(link_html.clone())]
        })?;
        let rel = to_php_string(&rel).unwrap_or_default();
        if !truthy(&rel) || !atts.iter().any(|a| &*a.name == b"target") {
            return Ok([&b"<a "[..], original, b">"].concat());
        }
        let mut wholes: Vec<Vec<u8>> = atts.iter().map(|a| a.whole.to_vec()).collect();
        let rel = match atts.iter().position(|a| &*a.name == b"rel") {
            Some(at) => {
                let joined = [&atts[at].value[..], b" ", &rel].concat();
                let flags = SplitFlags { delim_capture: false, no_empty: true };
                let parts = preg_split(&p.whitespace, &joined, -1, flags).unwrap_or_default();
                unique(parts.into_iter().map(<[u8]>::to_vec).collect()).join(&b' ')
            }
            None => rel,
        };
        let whole = [&b"rel=\""[..], &self.attr(&rel)?, b"\""].concat();
        match atts.iter().position(|a| &*a.name == b"rel") {
            Some(at) => wholes[at] = whole,
            None => wholes.push(whole),
        }
        let mut html = wholes.join(&b' ');
        if escaped {
            html = html.iter().fold(Vec::with_capacity(html.len()), |mut out, &b| {
                if b == b'\'' || b == b'"' {
                    out.push(b'\\');
                }
                out.push(b);
                out
            });
        }
        Ok([&b"<a "[..], &html, b">"].concat())
    }

    /// `wp_targeted_link_rel( $text )` (deprecated in 6.7.0): adds `rel="noopener"` to links with a `target`,
    /// outside `script` and `style`.
    ///
    /// @param text
    ///     The HTML.
    /// @return
    ///     The HTML.
    pub fn wp_targeted_link_rel(&self, text: &[u8]) -> Result<Vec<u8>, TextError> {
        let p = &*LINKS;
        self.d.diagnostics.deprecated_function("wp_targeted_link_rel", "6.7.0", "")?;
        let lower = text.to_ascii_lowercase();
        let has = |needle: &[u8]| lower.windows(needle.len()).any(|w| w == needle);
        if !has(b"target") || !has(b"<a ") || is_serialized(text) {
            return Ok(text.to_vec());
        }
        let extra = preg_match_all_group(&p.script_style, text, 0).unwrap_or_default();
        let parts = preg_split(&p.script_style, text, -1, SplitFlags { delim_capture: false, no_empty: false }).unwrap_or_default();
        let failure = Failure::new();
        let mut out = Vec::with_capacity(text.len());
        for (i, part) in parts.iter().enumerate() {
            let linked = replaced(preg_replace_callback(&p.targeted_anchor, part, |m| {
                failure.keep(self.wp_targeted_link_rel_callback(m.get(1).unwrap_or_default()), Vec::new())
            }));
            out.extend_from_slice(&linked);
            if let Some(extra) = extra.get(i) {
                out.extend_from_slice(extra);
            }
        }
        failure.finish(out)
    }

    /// `links_add_base_url( $content, $base, $attrs )`: relative URLs in the named attributes become absolute.
    ///
    /// @param content
    ///     The HTML.
    /// @param base
    ///     The base URL.
    /// @param attrs
    ///     The attribute names, joined into the pattern as they are (WordPress does not quote them).
    /// @return
    ///     The HTML.
    pub fn links_add_base_url(&self, content: &[u8], base: &[u8], attrs: &[&[u8]]) -> Result<Vec<u8>, TextError> {
        let source = [&b"!("[..], &attrs.join(&b'|'), b")=(['\"])(.+?)\\2!i"].concat();
        let Ok(pattern) = Pattern::try_php(&source) else {
            return Ok(Vec::new());
        };
        let failure = Failure::new();
        let out = replaced(preg_replace_callback(&pattern, content, |m| {
            let value = m.get(3).unwrap_or_default();
            let quote = m.get(2).unwrap_or_default();
            let kept = match preg_match(&LINKS.protocol, value) {
                Ok(Some(found)) => {
                    let scheme = found.get(1).unwrap_or_default();
                    failure.keep(
                        self.d.kses.allowed_protocols().map_err(kses_error).map(|p| p.0.iter().any(|q| &**q == scheme)),
                        false,
                    )
                }
                _ => false,
            };
            let url = if kept { value.to_vec() } else { make_absolute_url(value, base) };
            [m.get(1).unwrap_or_default(), b"=", quote, &url, quote].concat()
        }));
        failure.finish(out)
    }

    /// `links_add_target( $content, $target, $tags )`: sets `target` on the named tags.
    ///
    /// @param content
    ///     The HTML.
    /// @param target
    ///     The target.
    /// @param tags
    ///     The tag names, joined into the pattern as they are.
    /// @return
    ///     The HTML.
    pub fn links_add_target(&self, content: &[u8], target: &[u8], tags: &[&[u8]]) -> Result<Vec<u8>, TextError> {
        let source = [&b"!<("[..], &tags.join(&b'|'), b")((\\s[^>]*)?)>!i"].concat();
        let Ok(pattern) = Pattern::try_php(&source) else {
            return Ok(Vec::new());
        };
        let failure = Failure::new();
        let out = replaced(preg_replace_callback(&pattern, content, |m| {
            let link = replaced(preg_replace(&LINKS.target_attr, m.get(2).unwrap_or_default(), b"", None).map(|(o, _)| o));
            let escaped = failure.keep(self.attr(target), Vec::new());
            [&b"<"[..], m.get(1).unwrap_or_default(), &link, b" target=\"", &escaped, b"\">"].concat()
        }));
        failure.finish(out)
    }
}
