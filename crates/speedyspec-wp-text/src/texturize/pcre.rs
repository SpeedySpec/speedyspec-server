//! `wptexturize()` as WordPress runs it: its own patterns, on PCRE2.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use speedyspec_wp_shortcode_contracts::TagName;

use crate::preg::{Pattern, PregError, SplitFlags, preg_match, preg_match_all_group, preg_replace, preg_split};
use crate::strings::{PHP_TRIM_DEFAULT, php_trim, replace_one};

use super::primes::wptexturize_primes;
use super::tables::{APOS_FLAG, OPEN_Q_FLAG, OPEN_SQ_FLAG};
use super::{TexturizeScope, TexturizeTables, Texturizer, pushpop_element, texturize_shortcode_regex};

// formatting.php, wptexturize() and _get_wptexturize_split_regex().
static SHORTCODE_NAMES: LazyLock<Pattern> = LazyLock::new(|| Pattern::php(r"@\[/?([^<>&/\[\]\x00-\x20=]++)@"));
const HTML_REGEX: &str = r"<(?(?=!--)!(?:-(?!->)[^\-]*+)*+(?:-->)?|[^>]*>?)";
static SPLIT_HTML: LazyLock<Pattern> = LazyLock::new(|| Pattern::php(&format!("/({HTML_REGEX})/")));
static AMPERSAND: LazyLock<Pattern> =
    LazyLock::new(|| Pattern::php(r"/&(?!#(?:\d+|x[a-f0-9]+);|[a-z1-4]{1,8};)/i"));
static TIMES_PROBE: LazyLock<Pattern> = LazyLock::new(|| Pattern::php(r"/(?<=\d)x\d/"));
pub(crate) static TIMES: LazyLock<Pattern> = LazyLock::new(|| Pattern::php(r"/\b(\d(?(?<=0)[\d\.,]+|[\d\.,]*))x(\d[\d\.,]*)\b/"));

/// The split and match patterns for one set of shortcode names.
struct ShortcodePatterns {
    split: Pattern,
    whole: Pattern,
}

/// WordPress's `wptexturize()` loop, verbatim, on PCRE2. The reference for every other [`Texturizer`].
///
/// It keeps the compiled split pattern for each set of shortcode names it has seen.
#[derive(Default)]
pub struct PcreTexturizer {
    shortcodes: RefCell<HashMap<Vec<u8>, Rc<ShortcodePatterns>>>,
}

impl PcreTexturizer {
    fn shortcode_patterns(&self, found: &[TagName]) -> Rc<ShortcodePatterns> {
        let regex = texturize_shortcode_regex(found);
        if let Some(cached) = self.shortcodes.borrow().get(&regex) {
            return cached.clone();
        }
        let regex_str = String::from_utf8_lossy(&regex).into_owned();
        let patterns = Rc::new(ShortcodePatterns {
            split: Pattern::try_php(format!("/({HTML_REGEX}|{regex_str})/").as_bytes())
                .unwrap_or_else(|e| panic!("the texturize split pattern: {e}")),
            whole: Pattern::try_php(format!("/^{regex_str}$/").as_bytes())
                .unwrap_or_else(|e| panic!("the texturize shortcode pattern: {e}")),
        });
        self.shortcodes.borrow_mut().insert(regex, patterns.clone());
        patterns
    }
}

fn replace_all(patterns: &[(Pattern, Vec<u8>)], mut text: Vec<u8>) -> Result<Vec<u8>, PregError> {
    for (pattern, replacement) in patterns {
        let (out, count) = preg_replace(pattern, &text, replacement, None)?;
        if count > 0 {
            text = out.into_owned();
        }
    }
    Ok(text)
}

/// Replaces each `&` with `&#038;` unless it already looks like an entity.
pub(crate) fn ampersands(text: Vec<u8>) -> Result<Vec<u8>, PregError> {
    let (out, count) = preg_replace(&AMPERSAND, &text, b"&#038;", None)?;
    Ok(if count > 0 { out.into_owned() } else { text })
}

/// Texturizes one text chunk that is neither a tag nor inside a no-texturize element.
pub(crate) fn texturize_chunk(chunk: &[u8], tables: &TexturizeTables) -> Result<Vec<u8>, PregError> {
    let mut curl = tables.static_replace.apply(chunk).into_owned();
    if memchr::memchr(b'\'', &curl).is_some() {
        curl = replace_all(&tables.apos_patterns, curl)?;
        curl = wptexturize_primes(&curl, &tables.single_primes, &tables.prime, OPEN_SQ_FLAG, &tables.closing_single_quote)?;
        curl = replace_one(&curl, APOS_FLAG, &tables.apos).into_owned();
        curl = replace_one(&curl, OPEN_SQ_FLAG, &tables.opening_single_quote).into_owned();
    }
    if memchr::memchr(b'"', &curl).is_some() {
        curl = replace_all(&tables.quote_patterns, curl)?;
        curl = wptexturize_primes(&curl, &tables.double_primes, &tables.double_prime, OPEN_Q_FLAG, &tables.closing_quote)?;
        curl = replace_one(&curl, OPEN_Q_FLAG, &tables.opening_quote).into_owned();
    }
    if memchr::memchr(b'-', &curl).is_some() {
        curl = replace_all(&tables.dash_patterns, curl)?;
    }
    if preg_match(&TIMES_PROBE, &curl)?.is_some() {
        let (out, count) = preg_replace(&TIMES, &curl, b"$1&#215;$2", None)?;
        if count > 0 {
            curl = out.into_owned();
        }
    }
    ampersands(curl)
}

/// The registered names among those the text mentions, in registration order (`array_intersect`).
pub(crate) fn found_tags(text: &[u8], registered: &[TagName]) -> Result<Vec<TagName>, PregError> {
    if registered.is_empty() || memchr::memchr(b'[', text).is_none() {
        return Ok(Vec::new());
    }
    let mentioned = preg_match_all_group(&SHORTCODE_NAMES, text, 1)?;
    Ok(registered.iter().filter(|t| mentioned.contains(&t.as_bytes())).cloned().collect())
}

impl Texturizer for PcreTexturizer {
    fn texturize<'a>(
        &self,
        text: &'a [u8],
        tables: &TexturizeTables,
        scope: &TexturizeScope<'_>,
    ) -> Result<Cow<'a, [u8]>, PregError> {
        if text.is_empty() || text == b"0" {
            return Ok(Cow::Borrowed(text));
        }
        let found = found_tags(text, scope.shortcode_tags)?;
        let shortcodes = (!found.is_empty()).then(|| self.shortcode_patterns(&found));
        let split = shortcodes.as_ref().map_or(&*SPLIT_HTML, |s| &s.split);
        let flags = SplitFlags { delim_capture: true, no_empty: true };
        let chunks = preg_split(split, text, -1, flags)?;
        let mut tags_stack: Vec<Vec<u8>> = Vec::new();
        let mut shortcodes_stack: Vec<Vec<u8>> = Vec::new();
        let mut out = Vec::with_capacity(text.len() + text.len() / 8);
        for chunk in chunks {
            let first = chunk[0];
            if first == b'<' {
                if chunk.starts_with(b"<!--") {
                    out.extend_from_slice(chunk);
                    continue;
                }
                let element = ampersands(chunk.to_vec())?;
                pushpop_element(&element, &mut tags_stack, scope.no_texturize_tags);
                out.extend_from_slice(&element);
            } else if php_trim(chunk, PHP_TRIM_DEFAULT).is_empty() {
                out.extend_from_slice(chunk);
            } else if first == b'['
                && let Some(s) = &shortcodes
                && preg_match(&s.whole, chunk)?.is_some()
            {
                if !chunk.starts_with(b"[[") && !chunk.ends_with(b"]]") {
                    pushpop_element(chunk, &mut shortcodes_stack, scope.no_texturize_shortcodes);
                }
                out.extend_from_slice(chunk);
            } else if shortcodes_stack.is_empty() && tags_stack.is_empty() {
                out.extend_from_slice(&texturize_chunk(chunk, tables)?);
            } else {
                out.extend_from_slice(chunk);
            }
        }
        if out == text {
            return Ok(Cow::Borrowed(text));
        }
        Ok(Cow::Owned(out))
    }
}
