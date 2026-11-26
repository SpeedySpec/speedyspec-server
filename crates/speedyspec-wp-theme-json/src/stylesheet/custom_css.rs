//! `process_blocks_custom_css( $css, $selector )`: a block's custom CSS, with `&` nesting, under its selector.

use std::sync::LazyLock;

use fancy_regex::Regex;

use crate::selectors::{append_to_selector, scope_selector};

static PSEUDO_ELEMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([>+~ \t\n\x{0b}\x{0c}\r]*::[a-zA-Z-]+)").expect("compiles"));

/// A node's `css` as `process_blocks_custom_css()` takes it: only a string is CSS (WordPress 7.2).
pub(crate) fn css_text(css: &speedyspec_hooks::Value) -> String {
    match css {
        speedyspec_hooks::Value::Str(s) => s.clone(),
        speedyspec_hooks::Value::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
        _ => String::new(),
    }
}

/// PHP's `trim()` with its default characters.
fn php_trim(s: &str) -> &str {
    s.trim_matches([' ', '\t', '\n', '\r', '\0', '\u{b}'])
}

/// `WP_Theme_JSON::process_blocks_custom_css( $css, $selector )`.
pub fn process_blocks_custom_css(css: &str, selector: &str) -> String {
    let mut processed = String::new();
    if css.is_empty() || css == "0" {
        return processed;
    }
    for part in css.split('&') {
        if part.is_empty() || part == "0" {
            continue;
        }
        if !part.contains('{') {
            processed.push_str(&format!(
                ":root :where({}){{{}}}",
                php_trim(selector),
                php_trim(part)
            ));
            continue;
        }
        let without_close = part.replace('}', "");
        let pieces: Vec<&str> = without_close.split('{').collect();
        if pieces.len() != 2 {
            continue;
        }
        let mut nested = pieces[0].to_owned();
        let value = pieces[1];
        let pseudo = PSEUDO_ELEMENT
            .captures(nested.as_str())
            .ok()
            .flatten()
            .map(|c| c[1].to_owned())
            .unwrap_or_default();
        if !pseudo.is_empty() {
            nested = nested.replace(&pseudo, "");
        }
        let part_selector = if nested.starts_with(' ') {
            scope_selector(selector, &nested)
        } else {
            append_to_selector(selector, &nested)
        };
        processed.push_str(&format!(
            ":root :where({part_selector}){pseudo}{{{}}}",
            php_trim(value)
        ));
    }
    processed
}
