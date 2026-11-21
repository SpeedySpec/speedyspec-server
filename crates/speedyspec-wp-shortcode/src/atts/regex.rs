use regex::bytes::Regex;
use speedyspec_wp_shortcode_contracts::{AttsParse, AttsParser};

use super::shared::{Found, parse_with};

/// `shortcode_parse_atts()` with `get_shortcode_atts_regex()` on the `regex` crate's DFA fast paths.
///
/// The pattern has no lookarounds, so `regex` runs it as written, with ASCII classes and `$` as `\z`: the
/// whitespace branch before it already consumes a final newline.
pub struct RegexAttsParser {
    re: Regex,
}

impl Default for RegexAttsParser {
    fn default() -> Self {
        Self::new()
    }
}

impl RegexAttsParser {
    /// Compiles the pattern.
    pub fn new() -> Self {
        let s = r"[\t\n\x0B\x0C\r ]";
        let w = r"[A-Za-z0-9_-]";
        let end = format!(r"(?:{s}|\z)");
        let pattern = format!(
            r#"(?-u)({w}+){s}*={s}*"([^"]*)"{end}|({w}+){s}*={s}*'([^']*)'{end}|({w}+){s}*={s}*([^\t\n\x0B\x0C\r '"]+){end}|"([^"]*)"{end}|'([^']*)'{end}|([^\t\n\x0B\x0C\r ]+){end}"#
        );
        Self { re: Regex::new(&pattern).expect("the attribute pattern compiles") }
    }
}

impl AttsParser for RegexAttsParser {
    fn parse(&self, text: &[u8]) -> AttsParse {
        parse_with(text, |t| {
            self.re
                .captures_iter(t)
                .map(|c| {
                    let g = |i| c.get(i).map(|m| m.as_bytes());
                    if let (Some(n), Some(v)) = (g(1), g(2)) {
                        Found::Named(n, v)
                    } else if let (Some(n), Some(v)) = (g(3), g(4)) {
                        Found::Named(n, v)
                    } else if let (Some(n), Some(v)) = (g(5), g(6)) {
                        Found::Named(n, v)
                    } else if let Some(v) = g(7).or(g(8)) {
                        Found::Quoted(v)
                    } else {
                        Found::Bare(g(9).expect("one alternative matched"))
                    }
                })
                .collect()
        })
    }
}
