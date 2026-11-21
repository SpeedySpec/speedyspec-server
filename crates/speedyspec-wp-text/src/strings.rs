//! PHP's string functions, with the exact semantics WordPress relies on.

use std::borrow::Cow;

use aho_corasick::{AhoCorasick, MatchKind};
use memchr::memmem;
use speedyspec_hooks::Value as HookValue;

/// PHP's `str_replace(array $search, array $replace, string $subject)`.
///
/// PHP runs each pair over the whole result of the pair before, so a later search can match what an earlier
/// replacement wrote. One aho-corasick pass gives the same result only when no replacement contains any search
/// string and no two search strings can overlap; [`StrReplace::new`] checks that once and otherwise runs the pairs
/// in order. Empty search strings are skipped, as PHP skips them.
pub struct StrReplace {
    pairs: Vec<(Vec<u8>, Vec<u8>)>,
    one_pass: Option<AhoCorasick>,
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    memmem::find(haystack, needle).is_some()
}

/// Whether some proper suffix of `a` is a prefix of `b`, so a match of `a` and one of `b` can overlap.
fn overlaps(a: &[u8], b: &[u8]) -> bool {
    (1..a.len()).any(|i| b.starts_with(&a[i..]))
}

impl StrReplace {
    /// Prepares the pairs, in PHP's order.
    ///
    /// @param pairs
    ///     `(search, replace)` pairs.
    /// @return
    ///     The prepared replacement.
    pub fn new(pairs: Vec<(Vec<u8>, Vec<u8>)>) -> StrReplace {
        let pairs: Vec<_> = pairs.into_iter().filter(|(search, _)| !search.is_empty()).collect();
        let independent = pairs.iter().enumerate().all(|(i, (a, replace))| {
            pairs.iter().enumerate().all(|(j, (b, _))| {
                !contains(replace, b) && (i == j || (!contains(a, b) && !overlaps(a, b)))
            })
        });
        let one_pass = (independent && pairs.len() > 1).then(|| {
            AhoCorasick::builder()
                .match_kind(MatchKind::LeftmostFirst)
                .build(pairs.iter().map(|(s, _)| s))
                .expect("the search strings build an automaton")
        });
        StrReplace { pairs, one_pass }
    }

    /// Replaces in `text`.
    ///
    /// @param text
    ///     The subject.
    /// @return
    ///     The result, borrowed when nothing was found.
    pub fn apply<'a>(&self, text: &'a [u8]) -> Cow<'a, [u8]> {
        if let Some(automaton) = &self.one_pass {
            let mut found = automaton.find_iter(text).peekable();
            if found.peek().is_none() {
                return Cow::Borrowed(text);
            }
            let mut out = Vec::with_capacity(text.len() + text.len() / 8);
            let mut copied = 0;
            for m in found {
                out.extend_from_slice(&text[copied..m.start()]);
                out.extend_from_slice(&self.pairs[m.pattern().as_usize()].1);
                copied = m.end();
            }
            out.extend_from_slice(&text[copied..]);
            return Cow::Owned(out);
        }
        let mut current = Cow::Borrowed(text);
        for (search, replace) in &self.pairs {
            if let Cow::Owned(changed) = replace_one(&current, search, replace) {
                current = Cow::Owned(changed);
            }
        }
        current
    }
}

/// PHP's `str_replace()` with one search string: every non-overlapping occurrence, left to right.
///
/// @param text
///     The subject.
/// @param search
///     What to find; an empty string finds nothing.
/// @param replace
///     What to put in its place.
/// @return
///     The result, borrowed when `search` doesn't occur.
pub fn replace_one<'a>(text: &'a [u8], search: &[u8], replace: &[u8]) -> Cow<'a, [u8]> {
    if search.is_empty() {
        return Cow::Borrowed(text);
    }
    let mut found = memmem::find_iter(text, search).peekable();
    if found.peek().is_none() {
        return Cow::Borrowed(text);
    }
    let mut out = Vec::with_capacity(text.len());
    let mut copied = 0;
    for at in found {
        out.extend_from_slice(&text[copied..at]);
        out.extend_from_slice(replace);
        copied = at + search.len();
    }
    out.extend_from_slice(&text[copied..]);
    Cow::Owned(out)
}

/// The table for PHP's `strtr(string, array)`.
pub struct StrtrTable {
    replacements: Vec<Vec<u8>>,
    automaton: Option<AhoCorasick>,
}

impl StrtrTable {
    /// Prepares the pairs. An empty key is dropped, as PHP drops it.
    ///
    /// @param pairs
    ///     `(from, to)` pairs; a repeated key keeps its last value, as a PHP array would.
    /// @return
    ///     The table.
    pub fn new(pairs: Vec<(Vec<u8>, Vec<u8>)>) -> StrtrTable {
        let mut unique: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for (from, to) in pairs.into_iter().filter(|(from, _)| !from.is_empty()) {
            match unique.iter_mut().find(|(f, _)| *f == from) {
                Some(entry) => entry.1 = to,
                None => unique.push((from, to)),
            }
        }
        let automaton = (!unique.is_empty()).then(|| {
            AhoCorasick::builder()
                .match_kind(MatchKind::LeftmostLongest)
                .build(unique.iter().map(|(f, _)| f))
                .expect("the keys build an automaton")
        });
        StrtrTable { replacements: unique.into_iter().map(|(_, to)| to).collect(), automaton }
    }
}

/// PHP's `strtr(string, array)`: at each position the longest key wins, and replaced text is never rescanned.
///
/// @param text
///     The subject.
/// @param table
///     The prepared pairs.
/// @return
///     The result, borrowed when no key occurs.
pub fn strtr<'a>(text: &'a [u8], table: &StrtrTable) -> Cow<'a, [u8]> {
    let Some(automaton) = &table.automaton else {
        return Cow::Borrowed(text);
    };
    let mut found = automaton.find_iter(text).peekable();
    if found.peek().is_none() {
        return Cow::Borrowed(text);
    }
    let mut out = Vec::with_capacity(text.len());
    let mut copied = 0;
    for m in found {
        out.extend_from_slice(&text[copied..m.start()]);
        out.extend_from_slice(&table.replacements[m.pattern().as_usize()]);
        copied = m.end();
    }
    out.extend_from_slice(&text[copied..]);
    Cow::Owned(out)
}

/// The characters PHP's `trim()` strips by default.
pub const PHP_TRIM_DEFAULT: &[u8] = b" \t\n\r\0\x0B";

/// PHP's `trim()` with a list of characters (no `..` ranges).
///
/// @param text
///     The subject.
/// @param chars
///     The bytes to strip from both ends.
/// @return
///     The trimmed slice.
pub fn php_trim<'a>(text: &'a [u8], chars: &[u8]) -> &'a [u8] {
    let start = text.iter().position(|b| !chars.contains(b)).unwrap_or(text.len());
    let end = text.iter().rposition(|b| !chars.contains(b)).map_or(start, |i| i + 1);
    &text[start..end]
}

/// PHP's `(string)` conversion of a scalar.
///
/// @param value
///     The value.
/// @return
///     Its string form: `true` is `"1"`, `false` and `null` are `""`, floats use PHP's `precision` of 14 (`0.1`,
///     `1.0E+25`, `INF`); `None` for an array or an opaque value, which PHP can't convert silently.
pub fn to_php_string(value: &HookValue) -> Option<Vec<u8>> {
    match value {
        HookValue::Null | HookValue::Bool(false) => Some(Vec::new()),
        HookValue::Bool(true) => Some(b"1".to_vec()),
        HookValue::Int(i) => Some(i.to_string().into_bytes()),
        HookValue::Float(f) => Some(php_float(*f).into_bytes()),
        HookValue::Str(s) => Some(s.as_bytes().to_vec()),
        HookValue::Bytes(b) => Some(b.clone()),
        HookValue::Array(_) | HookValue::Opaque(_) => None,
    }
}

/// A float as PHP prints it with `precision=14` (`%.14G`, with `.0` added to a bare mantissa).
fn php_float(f: f64) -> String {
    if f.is_nan() {
        return "NAN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "INF" } else { "-INF" }.into();
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let scientific = format!("{f:.13e}");
    let (mantissa, exponent) = scientific.split_once('e').expect("a scientific float");
    let exponent: i32 = exponent.parse().expect("an exponent");
    let trim = |s: &str| -> String {
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s.to_string() }
    };
    if !(-4..14).contains(&exponent) {
        let mut mantissa = trim(mantissa);
        if !mantissa.contains('.') {
            mantissa.push_str(".0");
        }
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{mantissa}E{sign}{}", exponent.abs());
    }
    let decimals = (13 - exponent).max(0) as usize;
    trim(&format!("{f:.decimals$}"))
}

/// A byte string as the Hook API carries it: `Str` when it is UTF-8, `Bytes` otherwise.
///
/// @param bytes
///     The bytes.
/// @return
///     The value.
pub fn bytes_value(bytes: Vec<u8>) -> HookValue {
    match String::from_utf8(bytes) {
        Ok(s) => HookValue::Str(s),
        Err(e) => HookValue::Bytes(e.into_bytes()),
    }
}
