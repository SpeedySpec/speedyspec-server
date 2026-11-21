use std::cell::RefCell;
use std::rc::Rc;

use fancy_regex::{BytesMode, Regex, RegexBuilder};
use speedyspec_wp_shortcode_contracts::{
    ShortcodeError, ShortcodeMatch, ShortcodeMatches, ShortcodeScanner, TagName, TagSet,
};

/// The reference scanner: WordPress's `get_shortcode_regex()` run by `fancy-regex` on bytes.
///
/// It is the literal pattern, so it is the yardstick the hand scanner is fuzzed against. Tag names are escaped
/// as `\xHH` instead of with `preg_quote()`, because `fancy-regex` reads `\<` and `\>` as word boundaries.
#[derive(Default)]
pub struct FancyRegexScanner {
    cache: RefCell<Option<(u64, Vec<TagName>, Rc<Regex>)>>,
}

/// Far above PHP's `pcre.backtrack_limit`; the pattern's possessive groups keep real content well under it.
const BACKTRACK_LIMIT: usize = 10_000_000;

impl FancyRegexScanner {
    /// A scanner with an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    fn regex(&self, tags: &TagSet) -> Result<Rc<Regex>, ShortcodeError> {
        if let Some((generation, names, re)) = &*self.cache.borrow()
            && *generation == tags.generation()
            && names.as_slice() == tags.names()
        {
            return Ok(re.clone());
        }
        let re = Rc::new(
            RegexBuilder::new(&pattern(tags.names()))
                .bytes_mode(BytesMode::Ascii)
                .backtrack_limit(BACKTRACK_LIMIT)
                .build()
                .map_err(|e| ShortcodeError::ScanFailed(e.to_string()))?,
        );
        *self.cache.borrow_mut() = Some((tags.generation(), tags.names().to_vec(), re.clone()));
        Ok(re)
    }
}

fn pattern(names: &[TagName]) -> String {
    let mut alternation = String::new();
    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            alternation.push('|');
        }
        for &b in name.as_bytes() {
            if b.is_ascii_alphanumeric() || b == b'_' {
                alternation.push(b as char);
            } else {
                alternation.push_str(&format!("\\x{b:02X}"));
            }
        }
    }
    format!(
        r"\[(\[?)({alternation})(?![\w-])([^\]\/]*(?:\/(?!\])[^\]\/]*)*?)(?:(\/)\]|\](?:([^\[]*+(?:\[(?!\/\2\])[^\[]*+)*+)\[\/\2\])?)(\]?)"
    )
}

impl ShortcodeScanner for FancyRegexScanner {
    fn scan<'h>(&self, haystack: &'h [u8], tags: &TagSet) -> ShortcodeMatches<'h> {
        match self.regex(tags) {
            Ok(re) => Box::new(FancyMatches { re, haystack, pos: 0, done: false }),
            Err(e) => Box::new(std::iter::once(Err(e))),
        }
    }
}

struct FancyMatches<'h> {
    re: Rc<Regex>,
    haystack: &'h [u8],
    pos: usize,
    done: bool,
}

impl Iterator for FancyMatches<'_> {
    type Item = Result<ShortcodeMatch, ShortcodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.pos > self.haystack.len() {
            return None;
        }
        match self.re.captures_from_pos(self.haystack, self.pos) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).expect("group 0");
                let (start, end) = (whole.start(), whole.end());
                // The pattern starts with `\[`, so it never matches empty.
                self.pos = end;
                Some(Ok(ShortcodeMatch {
                    start,
                    end,
                    groups: std::array::from_fn(|i| caps.get(i).map(|m| m.start()..m.end())),
                }))
            }
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(ShortcodeError::ScanFailed(e.to_string())))
            }
        }
    }
}
