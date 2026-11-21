//! `safecss_filter_attr()`.

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_kses_contracts::KsesError;
use speedyspec_wp_kses_lexical::{bad_protocol, no_null};

use crate::KsesApi;
use crate::api::{bytes_value, php_string};

/// The properties `safe_style_css` starts from.
const SAFE_STYLE_CSS: &[&str] = &[
    "background",
    "background-color",
    "background-image",
    "background-position",
    "background-repeat",
    "background-size",
    "background-attachment",
    "background-blend-mode",
    "border",
    "border-radius",
    "border-width",
    "border-color",
    "border-style",
    "border-right",
    "border-right-color",
    "border-right-style",
    "border-right-width",
    "border-bottom",
    "border-bottom-color",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
    "border-bottom-style",
    "border-bottom-width",
    "border-bottom-right-radius",
    "border-bottom-left-radius",
    "border-left",
    "border-left-color",
    "border-left-style",
    "border-left-width",
    "border-top",
    "border-top-color",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-top-style",
    "border-top-width",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-spacing",
    "border-collapse",
    "caption-side",
    "columns",
    "column-count",
    "column-fill",
    "column-gap",
    "column-rule",
    "column-span",
    "column-width",
    "display",
    "color",
    "filter",
    "font",
    "font-family",
    "font-size",
    "font-style",
    "font-variant",
    "font-weight",
    "letter-spacing",
    "line-height",
    "text-align",
    "text-decoration",
    "text-indent",
    "text-transform",
    "white-space",
    "height",
    "min-height",
    "max-height",
    "width",
    "min-width",
    "max-width",
    "margin",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "margin-top",
    "margin-block-start",
    "margin-block-end",
    "margin-inline-start",
    "margin-inline-end",
    "padding",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "padding-top",
    "padding-block-start",
    "padding-block-end",
    "padding-inline-start",
    "padding-inline-end",
    "flex",
    "flex-basis",
    "flex-direction",
    "flex-flow",
    "flex-grow",
    "flex-shrink",
    "flex-wrap",
    "gap",
    "column-gap",
    "row-gap",
    "grid-template-columns",
    "grid-auto-columns",
    "grid-column-start",
    "grid-column-end",
    "grid-column",
    "grid-column-gap",
    "grid-template-rows",
    "grid-auto-rows",
    "grid-row-start",
    "grid-row-end",
    "grid-row",
    "grid-row-gap",
    "grid-gap",
    "justify-content",
    "justify-items",
    "justify-self",
    "align-content",
    "align-items",
    "align-self",
    "clear",
    "cursor",
    "direction",
    "float",
    "list-style-type",
    "object-fit",
    "object-position",
    "opacity",
    "overflow",
    "vertical-align",
    "writing-mode",
    "position",
    "top",
    "right",
    "bottom",
    "left",
    "z-index",
    "box-shadow",
    "aspect-ratio",
    "container-type",
    "fill",
    "fill-opacity",
    "fill-rule",
    "stroke",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "stroke-width",
    "color-interpolation",
    "color-interpolation-filters",
    "paint-order",
    "stop-color",
    "stop-opacity",
    "flood-color",
    "flood-opacity",
    "lighting-color",
    "marker",
    "marker-end",
    "marker-mid",
    "marker-start",
    "clip-path",
    "clip-rule",
    "mask",
    "mask-type",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "x",
    "y",
    "d",
    "alignment-baseline",
    "baseline-shift",
    "dominant-baseline",
    "glyph-orientation-horizontal",
    "glyph-orientation-vertical",
    "text-anchor",
    "unicode-bidi",
    "word-spacing",
    "font-size-adjust",
    "font-stretch",
    "color-rendering",
    "image-rendering",
    "shape-rendering",
    "text-rendering",
    "vector-effect",
    "transform",
    "transform-origin",
    "pointer-events",
    "visibility",
    "--*",
];

/// Properties whose values may hold `url()`.
const URL_DATA_TYPES: &[&[u8]] = &[
    b"background",
    b"background-image",
    b"cursor",
    b"filter",
    b"list-style",
    b"list-style-image",
    b"clip-path",
    b"fill",
    b"marker",
    b"marker-end",
    b"marker-mid",
    b"marker-start",
    b"mask",
    b"stroke",
];

/// Properties whose values may hold gradients.
const GRADIENT_DATA_TYPES: &[&[u8]] = &[b"background", b"background-image"];

/// The functions whose balanced arguments are removed before the final character test, in the regex's order.
const FUNCTIONS: &[&[u8]] = &[
    b"var",
    b"calc",
    b"min",
    b"max",
    b"minmax",
    b"clamp",
    b"repeat",
    b"matrix",
    b"matrix3d",
    b"perspective",
    b"rotate",
    b"rotate3d",
    b"rotateX",
    b"rotateY",
    b"rotateZ",
    b"scale",
    b"scale3d",
    b"scaleX",
    b"scaleY",
    b"scaleZ",
    b"skew",
    b"skewX",
    b"skewY",
    b"translate",
    b"translate3d",
    b"translateX",
    b"translateY",
    b"translateZ",
    b"circle",
    b"ellipse",
    b"inset",
    b"path",
    b"polygon",
    b"rect",
    b"shape",
    b"xywh",
];

/// PHP's `trim()`.
fn trim(s: &[u8]) -> &[u8] {
    let t = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0 | 0x0B);
    let start = s.iter().position(|b| !t(b)).unwrap_or(s.len());
    let end = s.iter().rposition(|b| !t(b)).map_or(start, |e| e + 1);
    &s[start..end]
}

fn replace_all(s: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    if from.is_empty() {
        return s.to_vec();
    }
    let mut out = Vec::with_capacity(s.len());
    let mut at = 0;
    while let Some(i) = memchr::memmem::find(&s[at..], from) {
        out.extend_from_slice(&s[at..at + i]);
        out.extend_from_slice(to);
        at += i + from.len();
    }
    out.extend_from_slice(&s[at..]);
    out
}

/// `preg_match_all( '/url\([^)]+\)/' )`.
fn url_matches(s: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = memchr::memmem::find(&s[at..], b"url(") {
        let start = at + i;
        let body = start + 4;
        match memchr::memchr(b')', &s[body..]) {
            Some(len) if len > 0 => {
                out.push(&s[start..body + len + 1]);
                at = body + len + 1;
            }
            _ => at = start + 1,
        }
    }
    out
}

/// `$css_test_string` after `str_replace( $url_match, '', ... )` for every match in order.
///
/// One `str_replace` per match is quadratic in the number of matches. When every `url(` in `item` starts one of the
/// matches and removing them all joins no new `url(`, each `str_replace` removes exactly the matches equal to its
/// needle, so cutting every match out in one pass gives the same string. Anything else takes PHP's way.
///
/// @param item
///     The trimmed declaration; `matches` are slices of it.
fn remove_url_matches(item: &[u8], matches: &[&[u8]]) -> Vec<u8> {
    let base = item.as_ptr() as usize;
    let mut starts = matches.iter().map(|m| m.as_ptr() as usize - base);
    let only_match_starts = memchr::memmem::find_iter(item, b"url(")
        .all(|at| starts.next() == Some(at))
        && starts.next().is_none();
    if only_match_starts {
        let mut cut = Vec::with_capacity(item.len());
        let mut at = 0;
        for m in matches {
            let start = m.as_ptr() as usize - base;
            cut.extend_from_slice(&item[at..start]);
            at = start + m.len();
        }
        cut.extend_from_slice(&item[at..]);
        if memchr::memmem::find(&cut, b"url(").is_none() {
            return cut;
        }
    }
    let mut test = item.to_vec();
    for m in matches {
        if memchr::memmem::find(&test, m).is_some() {
            test = replace_all(&test, m, b"");
        }
    }
    test
}

fn is_space(b: u8) -> bool {
    speedyspec_wp_kses_lexical::scan::is_space(b)
}

/// Group 2 of `/^url\(\s*(['"]?)(.*)(\g1)\s*\)$/` on one `url(...)`.
fn url_piece(m: &[u8]) -> &[u8] {
    let inner = &m[4..m.len() - 1];
    let lead = inner.iter().take_while(|b| is_space(**b)).count();
    let rest = &inner[lead..];
    if let Some(&q) = rest.first().filter(|q| **q == b'"' || **q == b'\'') {
        let body = &rest[1..];
        let mut end = body.len();
        while end > 0 {
            end -= 1;
            if body[end] == q && body[end + 1..].iter().all(|b| is_space(*b)) {
                return &body[..end];
            }
        }
    }
    rest
}

/// The end of the gradient match at `at`, per `(?:repeating-)?(?:linear|radial|conic)-gradient\((?:[^()]|\([^()]*\))*\)`.
fn gradient_end(s: &[u8], at: usize) -> Option<usize> {
    let mut p = at;
    if s[p..].starts_with(b"repeating-") {
        p += 10;
    }
    let name = [&b"linear"[..], b"radial", b"conic"]
        .into_iter()
        .find(|n| s[p..].starts_with(n))?;
    p += name.len();
    if !s[p..].starts_with(b"-gradient(") {
        return None;
    }
    p += 10;
    loop {
        match s.get(p)? {
            b')' => return Some(p + 1),
            b'(' => {
                let close = s[p + 1..].iter().position(|b| *b == b'(' || *b == b')')?;
                if s[p + 1 + close] != b')' {
                    return None;
                }
                p += close + 2;
            }
            _ => p += 1,
        }
    }
}

fn remove_gradients(s: &[u8]) -> Vec<u8> {
    let mut matches: Vec<Vec<u8>> = Vec::new();
    let mut at = 0;
    while at < s.len() {
        match gradient_end(s, at) {
            Some(end) => {
                matches.push(s[at..end].to_vec());
                at = end;
            }
            None => at += 1,
        }
    }
    let mut out = s.to_vec();
    for m in matches {
        out = replace_all(&out, &m, b"");
    }
    out
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The end of a balanced `(...)` starting at `s[at] == '('`.
fn balanced_end(s: &[u8], at: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, b) in s[at..].iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at + i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Removes every `\b(?:var|calc|...)(\((?:[^()]|(?1))*\))` match, left to right.
fn remove_functions(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut at = 0;
    let mut copied = 0;
    while at < s.len() {
        let boundary = is_word(s[at]) && (at == 0 || !is_word(s[at - 1]));
        if boundary {
            let found = FUNCTIONS.iter().find_map(|name| {
                (s[at..].starts_with(name) && s.get(at + name.len()) == Some(&b'('))
                    .then(|| balanced_end(s, at + name.len()))
                    .flatten()
            });
            if let Some(end) = found {
                out.extend_from_slice(&s[copied..at]);
                copied = end;
                at = end;
                continue;
            }
        }
        at += 1;
    }
    out.extend_from_slice(&s[copied..]);
    out
}

/// `/^--[a-zA-Z0-9-_]+$/`.
fn is_custom_property(s: &[u8]) -> bool {
    s.len() > 2
        && s.starts_with(b"--")
        && s[2..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
}

impl KsesApi {
    /// `safe_style_css`, as values: kses compares them with `in_array( ..., true )`, so only strings can match.
    fn safe_style_css(&self) -> Result<Vec<Value>, KsesError> {
        let defaults = || {
            Value::Array(
                SAFE_STYLE_CSS
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (Key::Int(i as i64), Value::Str((*s).into())))
                    .collect(),
            )
        };
        let list = self
            .filter("safe_style_css", defaults, Vec::new)?
            .unwrap_or_else(defaults);
        Ok(match list {
            Value::Array(items) => items.into_iter().map(|(_, v)| v).collect(),
            other => vec![other],
        })
    }

    /// `safecss_filter_attr()`: the declarations of an inline style that kses allows, joined by `;`.
    pub fn safecss_filter_attr(&self, css: &[u8]) -> Result<Vec<u8>, KsesError> {
        let css = no_null(css, false);
        let css: Vec<u8> = css
            .iter()
            .copied()
            .filter(|b| !matches!(b, b'\n' | b'\r' | b'\t'))
            .collect();
        let protocols = self.allowed_protocols()?;
        let mut allowed = self.safe_style_css()?;
        let is_string = |v: &Value, s: &[u8]| {
            matches!(v, Value::Str(_) | Value::Bytes(_)) && php_string(v).as_deref() == Some(s)
        };
        if allowed.is_empty() {
            return Ok(css);
        }
        let mut out: Vec<u8> = Vec::new();
        for raw in trim(&css).split(|b| *b == b';') {
            if raw.is_empty() {
                continue;
            }
            let item = trim(raw);
            let mut test = item.to_vec();
            let (mut found, mut url_attr, mut gradient_attr) = (false, false, false);
            let mut value: &[u8] = b"";
            match memchr::memchr(b':', item) {
                None => found = true,
                Some(colon) => {
                    let selector = trim(&item[..colon]);
                    value = &item[colon + 1..];
                    let custom = allowed.iter().any(|v| is_string(v, b"--*"))
                        && is_custom_property(selector);
                    if custom {
                        allowed.push(bytes_value(selector));
                    }
                    if allowed.iter().any(|v| is_string(v, selector)) {
                        found = true;
                        url_attr = URL_DATA_TYPES.contains(&selector);
                        gradient_attr = GRADIENT_DATA_TYPES.contains(&selector);
                    }
                    if custom {
                        let v = trim(value);
                        url_attr = v.starts_with(b"url(");
                        gradient_attr = memchr::memmem::find(v, b"-gradient(").is_some();
                    }
                }
            }
            if found && url_attr {
                let matches = url_matches(value);
                for m in &matches {
                    let piece = url_piece(m);
                    if piece.is_empty() || piece == b"0" {
                        found = false;
                        break;
                    }
                    let url = trim(piece);
                    if url.is_empty() || url == b"0" || *bad_protocol(url, &protocols) != *url {
                        found = false;
                        break;
                    }
                }
                if found {
                    test = remove_url_matches(item, &matches);
                }
            }
            if found && gradient_attr {
                test = remove_gradients(&test);
            }
            if found {
                test = remove_functions(&test);
                let forbidden = test
                    .iter()
                    .any(|b| matches!(b, b'\\' | b'(' | b'&' | b'=' | b'}'))
                    || memchr::memmem::find(&test, b"/*").is_some();
                let allow = match self.filter(
                    "safecss_filter_attr_allow_css",
                    || Value::Bool(!forbidden),
                    || vec![bytes_value(&test)],
                )? {
                    None => !forbidden,
                    Some(v) => truthy(&v),
                };
                if allow {
                    if !out.is_empty() {
                        out.push(b';');
                    }
                    out.extend_from_slice(item);
                }
            }
        }
        Ok(out)
    }
}

/// PHP's truthiness.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Int(i) => *i != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(_) | Value::Bytes(_) => !matches!(php_string(v).as_deref(), Some(b"" | b"0")),
        Value::Array(items) => !items.is_empty(),
        Value::Opaque(_) => true,
    }
}
