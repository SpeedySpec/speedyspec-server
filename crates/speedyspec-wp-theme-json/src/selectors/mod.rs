//! Selectors: WordPress's selector helpers (`split_selector_list`, `append_to_selector`, `prepend_to_selector`,
//! `scope_selector`, the block style variation selectors), `wp_get_block_css_selector` and the computed block
//! metadata. Selectors are only split, joined and prefixed, as WordPress does, so they stay strings.

mod block_css_selector;
mod metadata;

pub use block_css_selector::block_css_selector;
pub use metadata::{BlocksMetadataCache, compute_blocks_metadata, element_selectors};

/// PHP's `trim( $s, " \t\n" )`.
fn css_trim(s: &str) -> &str {
    s.trim_matches(|c| c == ' ' || c == '\t' || c == '\n')
}

/// PHP's `empty()` for a string.
fn empty_str(s: &str) -> bool {
    s.is_empty() || s == "0"
}

/// `strcspn( $s, $chars, $from )`: the index of the next byte in `chars`, or the length.
fn next_of(bytes: &[u8], chars: &[u8], from: usize) -> usize {
    bytes[from.min(bytes.len())..]
        .iter()
        .position(|b| chars.contains(b))
        .map_or(bytes.len(), |p| from + p)
}

/// `WP_Theme_JSON::split_selector_list()`: the top-level selectors of a list, each trimmed of CSS whitespace;
/// commas inside parentheses, strings, comments and escapes do not split.
pub fn split_selector_list(selector: &str) -> Vec<String> {
    if !selector.contains(',') {
        return vec![css_trim(selector).to_owned()];
    }
    let bytes = selector.as_bytes();
    let len = bytes.len();
    let mut selectors = Vec::new();
    let mut depth: i64 = 0;
    let mut at = 0;
    let mut was_at = 0;
    let piece =
        |from: usize, to: usize| css_trim(&String::from_utf8_lossy(&bytes[from..to])).to_owned();
    while at < len {
        let next_at = next_of(bytes, b"/,'\"()<-\\", at);
        if next_at >= len {
            break;
        }
        let cp = bytes[next_at];
        if cp == b'\\' {
            at = (next_at + 2).min(len);
            continue;
        }
        if cp == b'(' || cp == b')' {
            depth += if cp == b'(' { 1 } else { -1 };
            at = next_at + 1;
            continue;
        }
        if cp == b'\'' || cp == b'"' {
            let mut end = next_at + 1;
            while end < len {
                end = next_of(bytes, &[cp, b'\\'], end);
                if end >= len {
                    break;
                }
                let end_cp = bytes[end];
                if end_cp == b'\\' {
                    end += 2;
                    continue;
                }
                if end_cp == cp {
                    end += 1;
                    break;
                }
                end += 1;
            }
            at = end;
            continue;
        }
        if cp == b'/' && next_at + 1 < len && bytes[next_at + 1] == b'*' {
            at = match selector[next_at + 1..].find("*/") {
                Some(p) => next_at + 1 + p + 2,
                None => len,
            };
            continue;
        }
        if (cp == b'<' && bytes[next_at..].starts_with(b"<!--"))
            || (cp == b'-' && bytes[next_at..].starts_with(b"-->"))
        {
            at = next_at + if cp == b'<' { 4 } else { 3 };
            continue;
        }
        if cp == b',' && depth == 0 {
            selectors.push(piece(was_at, next_at));
            at = next_at + 1;
            was_at = at;
            continue;
        }
        at = next_at + 1;
    }
    if was_at < len {
        selectors.push(piece(was_at, len));
    }
    selectors
}

/// Whether the fast path applies: no comments, strings, functions, escapes or CDOs.
fn plain_list(selector: &str) -> bool {
    !selector.bytes().any(|b| b"/'\"(<\\".contains(&b))
}

/// `preg_replace( '~[ \t\n]*,[ \t\n]*~', $replacement, $s )`.
fn replace_commas(s: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ' ' || c == '\t' || c == '\n' {
            pending.push(c);
            continue;
        }
        if c == ',' {
            while chars
                .peek()
                .is_some_and(|n| *n == ' ' || *n == '\t' || *n == '\n')
            {
                chars.next();
            }
            pending.clear();
            out.push_str(replacement);
            continue;
        }
        out.push_str(&pending);
        pending.clear();
        out.push(c);
    }
    out.push_str(&pending);
    out
}

/// `WP_Theme_JSON::append_to_selector()`: `to_append` after each selector of the list.
pub fn append_to_selector(selector: &str, to_append: &str) -> String {
    if !selector.contains(',') {
        return format!("{}{to_append}", css_trim(selector));
    }
    if plain_list(selector) {
        return format!(
            "{}{to_append}",
            replace_commas(css_trim(selector), &format!("{to_append}, "))
        );
    }
    split_selector_list(selector)
        .iter()
        .map(|s| format!("{s}{to_append}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `WP_Theme_JSON::prepend_to_selector()`: `to_prepend` before each selector of the list.
pub fn prepend_to_selector(selector: &str, to_prepend: &str) -> String {
    if !selector.contains(',') {
        return format!("{to_prepend}{}", css_trim(selector));
    }
    if plain_list(selector) {
        return format!(
            "{to_prepend}{}",
            replace_commas(css_trim(selector), &format!(", {to_prepend}"))
        );
    }
    split_selector_list(selector)
        .iter()
        .map(|s| format!("{to_prepend}{s}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `WP_Theme_JSON::scope_selector( $scope, $selector )`: every selector of the list under every scope.
pub fn scope_selector(scope: &str, selector: &str) -> String {
    if empty_str(scope) || empty_str(selector) {
        return selector.to_owned();
    }
    let scopes = split_selector_list(scope);
    let selectors = split_selector_list(selector);
    let mut scoped = Vec::new();
    for outer in &scopes {
        for inner in &selectors {
            if !empty_str(outer) && !empty_str(inner) {
                scoped.push(format!("{outer} {inner}"));
            } else if empty_str(outer) {
                scoped.push(inner.clone());
            } else if empty_str(inner) {
                scoped.push(outer.clone());
            }
        }
    }
    scoped.join(", ")
}

/// PCRE's `\s` without `/u`.
fn pcre_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
}

/// `WP_Theme_JSON::get_block_style_variation_selector()`: `.is-style-<name>` after the first compound of each
/// selector of the block's list.
pub fn block_style_variation_selector(variation: &str, block_selector: &str) -> String {
    let class = format!(".is-style-{variation}");
    if empty_str(block_selector) {
        return class;
    }
    split_selector_list(block_selector)
        .iter()
        .map(|part| {
            let start = part.find(|c: char| !pcre_space(c) && c != ':');
            match start {
                None => part.clone(),
                Some(start) => {
                    let end = part[start..]
                        .find(|c: char| pcre_space(c) || c == ':')
                        .map_or(part.len(), |e| start + e);
                    format!("{}{class}{}", &part[..end], &part[end..])
                }
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// `WP_Theme_JSON::get_block_style_variation_feature_selector( $style_variation, $feature_selector )`, given the
/// variation's name (its `name`, else the last step of its `path`) and its `selector`.
pub fn block_style_variation_feature_selector(
    name: Option<&str>,
    variation_selector: Option<&str>,
    feature_selector: &str,
) -> String {
    let Some(name) = name.filter(|n| !empty_str(n)) else {
        return variation_selector.unwrap_or(feature_selector).to_owned();
    };
    let prefix = format!(".is-style-{name} ");
    let parts: Vec<String> = split_selector_list(feature_selector)
        .into_iter()
        .map(|s| match s.strip_prefix(&prefix) {
            Some(rest) => rest.to_owned(),
            None => s,
        })
        .collect();
    block_style_variation_selector(name, &parts.join(", "))
}
