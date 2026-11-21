//! `WP_Token_Map`, ported line by line on WordPress's own layout: a `groups` string of `key_length`-byte prefixes
//! each followed by a NUL, one packed string per group of the longer words (`[length][suffix][length][mapping]`,
//! longest first then alphabetical), and the short words padded with NULs to `key_length + 1` bytes beside their
//! mappings. Keeping the layout keeps every behaviour, quirks included.

/// The longest word or mapping WordPress accepts, exclusive.
pub const MAX_LENGTH: usize = 256;

/// The storage version of a precomputed table.
pub const STORAGE_VERSION: &str = "6.6.0-trunk";

/// How words compare, as WordPress's `$case_sensitivity` argument names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseSensitivity {
    /// `'case-sensitive'` (and any string other than the next).
    CaseSensitive,
    /// `'ascii-case-insensitive'`.
    AsciiCaseInsensitive,
}

impl CaseSensitivity {
    /// The mode a PHP argument names: only `ascii-case-insensitive` ignores case, as WordPress compares.
    pub fn from_php(name: &[u8]) -> CaseSensitivity {
        if name == b"ascii-case-insensitive" {
            CaseSensitivity::AsciiCaseInsensitive
        } else {
            CaseSensitivity::CaseSensitive
        }
    }
}

/// A word or mapping of [`MAX_LENGTH`] bytes or more: WordPress reports `_doing_it_wrong()` and returns null.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooLong;

/// The packed groups or the small mappings: owned strings, or a compile-time table's.
#[derive(Clone, Copy, Debug)]
pub enum Words<'a> {
    Owned(&'a [Vec<u8>]),
    Static(&'a [&'a [u8]]),
}

impl<'a> Words<'a> {
    /// Every string, in order.
    pub fn slices(&self) -> Vec<&'a [u8]> {
        (0..self.len()).map(|i| self.get(i)).collect()
    }

    fn get(&self, i: usize) -> &'a [u8] {
        match self {
            Words::Owned(w) => &w[i],
            Words::Static(w) => w[i],
        }
    }

    fn len(&self) -> usize {
        match self {
            Words::Owned(w) => w.len(),
            Words::Static(w) => w.len(),
        }
    }
}

/// A token map's state, borrowed: what `WP_Token_Map`'s private properties hold.
#[derive(Clone, Copy, Debug)]
pub struct TokenMapView<'a> {
    pub key_length: usize,
    pub groups: &'a [u8],
    pub large_words: Words<'a>,
    pub small_words: &'a [u8],
    pub small_mappings: Words<'a>,
}

fn eq_bytes(a: &[u8], b: &[u8], ignore_case: bool) -> bool {
    if ignore_case {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// `strpos()` / `stripos()`: the first position of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8], ignore_case: bool) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|w| eq_bytes(w, needle, ignore_case))
}

/// `0 === substr_compare( $main, $str, $offset, strlen( $str ), $ignore_case )` for an offset within `$main`.
fn compare_at(main: &[u8], offset: usize, s: &[u8], ignore_case: bool) -> bool {
    main.get(offset..offset + s.len())
        .is_some_and(|w| eq_bytes(w, s, ignore_case))
}

impl<'a> TokenMapView<'a> {
    /// `contains( $word, $case_sensitivity )`.
    pub fn contains(&self, word: &[u8], case: CaseSensitivity) -> bool {
        if word.contains(&0) {
            return false;
        }
        let ignore_case = case == CaseSensitivity::AsciiCaseInsensitive;
        let kl = self.key_length;
        if kl >= word.len() {
            if self.small_words.is_empty() {
                return false;
            }
            let mut term = word.to_vec();
            term.resize(kl + 1, 0);
            return find(self.small_words, &term, ignore_case).is_some();
        }
        let Some(group_at) = find(self.groups, &word[..kl], ignore_case) else {
            return false;
        };
        let group = self.large_words.get(group_at / (kl + 1));
        let slug = &word[kl..];
        let mut at = 0;
        while at < group.len() {
            let token_length = group[at] as usize;
            at += 1;
            let token_at = at;
            at += token_length;
            let mapping_length = group[at] as usize;
            at += 1;
            if token_length == slug.len() && compare_at(group, token_at, slug, ignore_case) {
                return true;
            }
            at += mapping_length;
        }
        false
    }

    /// `read_token( $text, $offset, $matched_token_byte_length, $case_sensitivity )`: the mapping and the matched
    /// token's byte length, or `None` for WordPress's null.
    pub fn read_token(
        &self,
        text: &[u8],
        offset: usize,
        case: CaseSensitivity,
    ) -> Option<(&'a [u8], usize)> {
        let ignore_case = case == CaseSensitivity::AsciiCaseInsensitive;
        let kl = self.key_length;
        if offset > text.len() {
            return None;
        }
        if text.len() > kl {
            let window = &text[offset..(offset + kl).min(text.len())];
            // strcspn( $text, "\x00", $offset, $key_length ) < $key_length.
            let before_nul = window.iter().position(|b| *b == 0).unwrap_or(window.len());
            if before_nul < kl {
                return None;
            }
            match find(self.groups, window, ignore_case) {
                None => return self.read_small_token(text, offset, ignore_case),
                Some(group_at) => {
                    let group = self.large_words.get(group_at / (kl + 1));
                    let mut at = 0;
                    while at < group.len() {
                        let token_length = group[at] as usize;
                        at += 1;
                        let token = &group[at..at + token_length];
                        at += token_length;
                        let mapping_length = group[at] as usize;
                        at += 1;
                        let mapping_at = at;
                        if compare_at(text, offset + kl, token, ignore_case) {
                            return Some((
                                &group[mapping_at..mapping_at + mapping_length],
                                kl + token_length,
                            ));
                        }
                        at = mapping_at + mapping_length;
                    }
                }
            }
        }
        self.read_small_token(text, offset, ignore_case)
    }

    /// `read_small_token()`, with WordPress's comparison of later bytes (its `strtoupper()` wraps a boolean, so
    /// they compare case-sensitively even when case is ignored).
    fn read_small_token(
        &self,
        text: &[u8],
        offset: usize,
        ignore_case: bool,
    ) -> Option<(&'a [u8], usize)> {
        if self.small_words.is_empty() {
            return None;
        }
        let kl = self.key_length;
        let mut search = text[offset..(offset + kl).min(text.len())].to_vec();
        if ignore_case {
            search.make_ascii_uppercase();
        }
        // `$search_text[0]` of an empty string is '' (with a warning), which matches nothing.
        let starting = *search.first()?;
        let mut at = 0;
        'words: while at < self.small_words.len() {
            let first = self.small_words[at];
            if starting != first && (!ignore_case || first.to_ascii_uppercase() != starting) {
                at += kl + 1;
                continue;
            }
            for adjust in 1..kl {
                let byte = self.small_words[at + adjust];
                if byte == 0 {
                    return Some((self.small_mappings.get(at / (kl + 1)), adjust));
                }
                if search.get(adjust) != Some(&byte) {
                    at += kl + 1;
                    continue 'words;
                }
            }
            return Some((self.small_mappings.get(at / (kl + 1)), kl.max(1)));
        }
        None
    }

    /// `to_array()`: every word and its mapping, the short words first. As in WordPress, a group's prefix is read
    /// as two bytes whatever the key length.
    pub fn to_array(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        let kl = self.key_length;
        let mut tokens: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let mut put =
            |key: Vec<u8>, value: Vec<u8>| match tokens.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => tokens.push((key, value)),
            };
        let mut at = 0;
        let mut small = 0;
        while at < self.small_words.len() {
            let entry = &self.small_words[at..(at + kl + 1).min(self.small_words.len())];
            let end = entry.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1);
            put(
                entry[..end].to_vec(),
                self.small_mappings.get(small).to_vec(),
            );
            small += 1;
            at += kl + 1;
        }
        for index in 0..self.large_words.len() {
            let start = (index * (kl + 1)).min(self.groups.len());
            let prefix = &self.groups[start..(start + 2).min(self.groups.len())];
            let group = self.large_words.get(index);
            let mut at = 0;
            while at < group.len() {
                let length = group[at] as usize;
                at += 1;
                let mut key = prefix.to_vec();
                key.extend_from_slice(&group[at..at + length]);
                at += length;
                let length = group[at] as usize;
                at += 1;
                put(key, group[at..at + length].to_vec());
                at += length;
            }
        }
        tokens
    }
}

/// A token map built at run time (`WP_Token_Map::from_array()`), owning its state.
#[derive(Clone, Debug, Default)]
pub struct TokenMap {
    key_length: usize,
    groups: Vec<u8>,
    large_words: Vec<Vec<u8>>,
    small_words: Vec<u8>,
    small_mappings: Vec<Vec<u8>>,
}

/// PHP 8's `is_numeric()` for a string.
fn numeric(s: &[u8]) -> Option<f64> {
    let t = std::str::from_utf8(s).ok()?;
    // Leading and (since PHP 8) trailing whitespace are allowed.
    let trimmed = t.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0B' | '\x0C'));
    if trimmed.is_empty() {
        return None;
    }
    let body = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], Some(&body[i + 1..])),
        None => (body, None),
    };
    let digits = mantissa.replace('.', "");
    if digits.is_empty()
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || mantissa.matches('.').count() > 1
    {
        return None;
    }
    if let Some(e) = exponent {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        if e.is_empty() || !e.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    trimmed.parse().ok()
}

/// `sort()`'s comparison of two group keys: numerically when both are numeric strings, else bytewise.
fn php_sort_cmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    match (numeric(a), numeric(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        _ => a.cmp(b),
    }
}

/// A word (or suffix) and its mapping.
type Pair = (Vec<u8>, Vec<u8>);

/// `longest_first_then_alphabetical()`.
fn longest_first(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    b.len().cmp(&a.len()).then(a.cmp(b))
}

impl TokenMap {
    /// `WP_Token_Map::from_array( $mappings, $key_length )`, the words in the array's order.
    pub fn from_array(
        mappings: &[(Vec<u8>, Vec<u8>)],
        key_length: usize,
    ) -> Result<TokenMap, TooLong> {
        let mut groups: Vec<(Vec<u8>, Vec<Pair>)> = Vec::new();
        let mut shorts: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for (word, mapping) in mappings {
            if word.len() >= MAX_LENGTH || mapping.len() >= MAX_LENGTH {
                return Err(TooLong);
            }
            if key_length >= word.len() {
                shorts.push((word.clone(), mapping.clone()));
            } else {
                let key = word[..key_length].to_vec();
                let entry = (word[key_length..].to_vec(), mapping.clone());
                match groups.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, list)) => list.push(entry),
                    None => groups.push((key, vec![entry])),
                }
            }
        }
        shorts.sort_by(|a, b| longest_first(&a.0, &b.0));
        for (_, list) in &mut groups {
            list.sort_by(|a, b| longest_first(&a.0, &b.0));
        }
        let mut map = TokenMap {
            key_length,
            ..TokenMap::default()
        };
        for (word, mapping) in &shorts {
            let mut padded = word.clone();
            padded.resize(key_length + 1, 0);
            map.small_words.extend_from_slice(&padded);
            map.small_mappings.push(mapping.clone());
        }
        groups.sort_by(|a, b| php_sort_cmp(&a.0, &b.0));
        for (key, list) in &groups {
            map.groups.extend_from_slice(key);
            map.groups.push(0);
            let mut group = Vec::new();
            for (word, mapping) in list {
                group.push(word.len() as u8);
                group.extend_from_slice(word);
                group.push(mapping.len() as u8);
                group.extend_from_slice(mapping);
            }
            map.large_words.push(group);
        }
        Ok(map)
    }

    /// The map's state, for lookups.
    pub fn view(&self) -> TokenMapView<'_> {
        TokenMapView {
            key_length: self.key_length,
            groups: &self.groups,
            large_words: Words::Owned(&self.large_words),
            small_words: &self.small_words,
            small_mappings: Words::Owned(&self.small_mappings),
        }
    }
}
