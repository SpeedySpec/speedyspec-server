//! `do_shortcodes_in_html_tags()`, line for line.

use speedyspec_wp_shortcode_contracts::{ShortcodeError, TagSet};

use crate::api::{Callback, ShortcodeApi};
use crate::atts::is_pcre_space;
use crate::strtr::{encode_brackets, normalize_entities};

/// `preg_match( '%^<\s*\[\[?[^\[\]]+\]%', $element )`.
fn looks_like_bracketed_email(e: &[u8]) -> bool {
    let mut i = 1;
    if e.first() != Some(&b'<') {
        return false;
    }
    while i < e.len() && is_pcre_space(e[i]) {
        i += 1;
    }
    if e.get(i) != Some(&b'[') {
        return false;
    }
    i += 1;
    if e.get(i) == Some(&b'[') {
        i += 1;
    }
    let start = i;
    while i < e.len() && e[i] != b'[' && e[i] != b']' {
        i += 1;
    }
    i > start && e.get(i) == Some(&b']')
}

/// The first `[a-zA-Z0-9]+` in `front`: the element name.
fn element_name(front: &[u8]) -> &[u8] {
    let Some(start) = front.iter().position(u8::is_ascii_alphanumeric) else {
        return b"";
    };
    let len = front[start..].iter().take_while(|b| b.is_ascii_alphanumeric()).count();
    &front[start..start + len]
}

/// PHP's `trim()` with its default characters.
fn php_trim(s: &[u8]) -> &[u8] {
    let ws = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0 | 0x0B);
    let start = s.iter().position(|b| !ws(b)).unwrap_or(s.len());
    let end = s.iter().rposition(|b| !ws(b)).map_or(start, |e| e + 1);
    &s[start..end]
}

impl ShortcodeApi {
    /// `do_shortcodes_in_html_tags( $content, $ignore_html, $tagnames )`.
    ///
    /// # Errors
    ///
    /// Any error from a handler, a filter or the scanner.
    pub fn do_shortcodes_in_html_tags(
        &self,
        content: &[u8],
        ignore_html: bool,
        tags: &TagSet,
    ) -> Result<Vec<u8>, ShortcodeError> {
        // `get_shortcode_regex( array() )` falls back to every registered name, as `empty( $tagnames )` is true.
        let registered;
        let tags = if tags.names().is_empty() {
            registered = TagSet::new(self.deps().registry.tag_names(), self.deps().registry.generation());
            &registered
        } else {
            tags
        };
        let content = normalize_entities(content);
        let mut out = Vec::with_capacity(content.len());
        for element in self.deps().html.split(&content) {
            if element.first() != Some(&b'<') {
                out.extend_from_slice(element);
                continue;
            }
            let noopen = memchr::memchr(b'[', element).is_none();
            let noclose = memchr::memchr(b']', element).is_none();
            if noopen || noclose {
                if noopen != noclose {
                    out.extend(encode_brackets(element));
                } else {
                    out.extend_from_slice(element);
                }
                continue;
            }
            if ignore_html || element.starts_with(b"<!--") || element.starts_with(b"<![CDATA[") {
                out.extend(encode_brackets(element));
                continue;
            }
            let Some(attributes) = self.deps().kses.attr_parse(element) else {
                let mut e = element.to_vec();
                if looks_like_bracketed_email(element) {
                    e = self.replace(&e, tags, Callback::Do)?.0;
                }
                out.extend(encode_brackets(&e));
                continue;
            };
            let (front, middle, back): (&[u8], &[&[u8]], &[u8]) = match attributes.as_slice() {
                [] => (b"", &[], b""),
                [only] => (only, &[], b""),
                [front, middle @ .., back] => (front, middle, back),
            };
            let elname = element_name(front);
            let mut rebuilt = front.to_vec();
            for &attr in middle {
                let (Some(open), Some(_)) = (memchr::memchr(b'[', attr), memchr::memchr(b']', attr)) else {
                    rebuilt.extend_from_slice(attr);
                    continue;
                };
                let double = memchr::memchr(b'"', attr);
                let single = memchr::memchr(b'\'', attr);
                if single.is_none_or(|s| open < s) && double.is_none_or(|d| open < d) {
                    rebuilt.extend(self.replace(attr, tags, Callback::Do)?.0);
                } else {
                    let (new_attr, count) = self.replace(attr, tags, Callback::Do)?;
                    if count > 0 {
                        let new_attr = self.deps().kses.one_attr(&new_attr, elname)?;
                        if !php_trim(&new_attr).is_empty() {
                            rebuilt.extend(new_attr);
                            continue;
                        }
                    }
                    rebuilt.extend_from_slice(attr);
                }
            }
            rebuilt.extend_from_slice(back);
            out.extend(encode_brackets(&rebuilt));
        }
        Ok(out)
    }
}
