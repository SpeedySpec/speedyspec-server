//! PHP's `preg_*` functions over PCRE2, the engine PHP itself uses.
//!
//! WordPress's formatting functions are mostly regular expressions, and their exact output depends on PCRE's
//! semantics: possessive quantifiers, conditionals, lookbehind with alternatives, byte mode. Running WordPress's
//! patterns unchanged on PCRE2 keeps those semantics; this module adds PHP's pattern syntax (delimiters and
//! modifiers), its replacement syntax and its split rules on top.
//!
//! One simplification: after an empty match at a position, the next search starts one character later. PHP first
//! retries at the same position for a non-empty match. No WordPress pattern ported here can match empty.

use std::borrow::Cow;
use std::fmt;

use pcre2::bytes::{CaptureLocations, Regex, RegexBuilder};

/// A compiled PHP pattern, such as `/a(b)?/i`.
pub struct Pattern {
    regex: Regex,
    utf: bool,
}

/// What PHP's `preg_*` functions report by returning `null` or `false`: a pattern that won't compile, or a match
/// that fails (invalid UTF-8 under `u`, a resource limit).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PregError {
    pub message: String,
}

impl fmt::Display for PregError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PregError {}

fn error(message: impl fmt::Display) -> PregError {
    PregError { message: message.to_string() }
}

impl Pattern {
    /// Compiles a constant PHP pattern literal, delimiters and modifiers included.
    ///
    /// @param source
    ///     The pattern as WordPress writes it, for example `"@<(script|style)[^>]*?>.*?</\\1>@si"`.
    /// @return
    ///     The compiled pattern.
    /// Panics when the pattern doesn't compile: every caller passes a constant copied from WordPress.
    pub fn php(source: &str) -> Pattern {
        Pattern::try_php(source.as_bytes()).unwrap_or_else(|e| panic!("the pattern {source} is not valid: {e}"))
    }

    /// Compiles a PHP pattern literal that may come from a filter, and so may be invalid.
    ///
    /// @param source
    ///     The pattern, delimiters and modifiers included, as bytes.
    /// @return
    ///     The compiled pattern, or the error PHP would warn about.
    pub fn try_php(source: &[u8]) -> Result<Pattern, PregError> {
        let start = source.iter().position(|b| !b.is_ascii_whitespace()).ok_or_else(|| error("empty regular expression"))?;
        let delimiter = source[start];
        if delimiter.is_ascii_alphanumeric() || delimiter == b'\\' {
            return Err(error("delimiter must not be alphanumeric or backslash"));
        }
        let closer = match delimiter {
            b'(' => b')',
            b'[' => b']',
            b'{' => b'}',
            b'<' => b'>',
            other => other,
        };
        let body_start = start + 1;
        let mut i = body_start;
        let mut depth = 1usize;
        let end = loop {
            let Some(&b) = source.get(i) else {
                return Err(error(format!("no ending delimiter '{}' found", closer as char)));
            };
            if b == b'\\' {
                i += 2;
                continue;
            }
            if closer != delimiter && b == delimiter {
                depth += 1;
            } else if b == closer {
                depth -= 1;
                if depth == 0 {
                    break i;
                }
            }
            i += 1;
        };
        let body = &source[body_start..end];
        let mut builder = RegexBuilder::new();
        builder.jit_if_available(true);
        let mut utf = false;
        let mut prefix = String::new();
        for &m in &source[end + 1..] {
            match m {
                b'i' => drop(builder.caseless(true)),
                b'm' => drop(builder.multi_line(true)),
                b's' => drop(builder.dotall(true)),
                b'x' => drop(builder.extended(true)),
                b'u' => {
                    utf = true;
                    builder.utf(true).ucp(true);
                }
                b'U' => prefix.push_str("(?U)"),
                b'\n' | b'\r' | b' ' => {}
                other => return Err(error(format!("unknown or unsupported modifier '{}'", other as char))),
            }
        }
        let text = match (utf, std::str::from_utf8(body)) {
            (true, Ok(text)) => text.to_string(),
            (true, Err(_)) => return Err(error("the pattern is not valid UTF-8")),
            (false, _) => {
                // Byte mode: a non-ASCII byte stands for itself, which PCRE2 reads from `\xHH`.
                let mut text = String::with_capacity(body.len());
                for &b in body {
                    if b.is_ascii() {
                        text.push(b as char);
                    } else {
                        text.push_str(&format!("\\x{b:02X}"));
                    }
                }
                text
            }
        };
        let regex = builder.build(&(prefix + &text)).map_err(error)?;
        Ok(Pattern { regex, utf })
    }

    /// The next match at or after `from`, as capture locations; text before `from` is seen by lookbehind.
    fn next(&self, subject: &[u8], from: usize, locations: &mut CaptureLocations) -> Result<bool, PregError> {
        Ok(self.regex.captures_read_at(locations, subject, from).map_err(error)?.is_some())
    }

    /// Where the search resumes after an empty match at `at`: the next character.
    fn step(&self, subject: &[u8], at: usize) -> usize {
        if self.utf {
            let mut next = at + 1;
            while next < subject.len() && subject[next] & 0xC0 == 0x80 {
                next += 1;
            }
            next
        } else {
            at + 1
        }
    }

    /// PHP checks a `u` pattern's subject once per call and fails on invalid UTF-8. PCRE2 here is built with
    /// `PCRE2_MATCH_INVALID_UTF`, which would just not match, so the check is done here.
    fn check(&self, subject: &[u8]) -> Result<(), PregError> {
        if self.utf && !speedyspec_wp_utf8::is_valid_utf8(subject) {
            return Err(error("Malformed UTF-8 characters, possibly incorrectly encoded"));
        }
        Ok(())
    }

    fn groups(&self) -> usize {
        self.regex.captures_len()
    }
}

/// One match and its groups.
pub struct Captures<'a> {
    subject: &'a [u8],
    spans: Vec<Option<(usize, usize)>>,
}

impl<'a> Captures<'a> {
    fn read(subject: &'a [u8], locations: &CaptureLocations, groups: usize) -> Captures<'a> {
        Captures { subject, spans: (0..groups).map(|i| locations.get(i)).collect() }
    }

    /// Group `n`'s bytes; `None` when the group didn't take part in the match (PHP gives `""`).
    pub fn get(&self, n: usize) -> Option<&'a [u8]> {
        self.spans.get(n).copied().flatten().map(|(s, e)| &self.subject[s..e])
    }

    /// The whole match.
    pub fn full(&self) -> &'a [u8] {
        self.get(0).unwrap_or_default()
    }

    /// Where the whole match starts in the subject.
    pub fn start(&self) -> usize {
        self.spans[0].map_or(0, |(s, _)| s)
    }

    /// Where the whole match ends in the subject.
    pub fn end(&self) -> usize {
        self.spans[0].map_or(0, |(_, e)| e)
    }

    /// How many groups the pattern has, the whole match included.
    pub fn len(&self) -> usize {
        self.spans.len()
    }

    /// Whether the pattern has no groups at all, which never happens: the whole match is group 0.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }
}

/// Visits every non-overlapping match in order, as PHP's global functions do.
fn for_each_match<'a>(
    pattern: &Pattern,
    subject: &'a [u8],
    limit: Option<usize>,
    mut visit: impl FnMut(Captures<'a>),
) -> Result<usize, PregError> {
    pattern.check(subject)?;
    let mut locations = pattern.regex.capture_locations();
    let groups = pattern.groups();
    let mut from = 0;
    let mut count = 0;
    while from <= subject.len() && limit.is_none_or(|l| count < l) {
        if !pattern.next(subject, from, &mut locations)? {
            break;
        }
        let (start, end) = locations.get(0).expect("a match has group 0");
        visit(Captures::read(subject, &locations, groups));
        count += 1;
        from = if end == start { pattern.step(subject, end) } else { end };
    }
    Ok(count)
}

/// A parsed PHP replacement string.
enum Piece {
    Literal(Vec<u8>),
    Group(usize),
}

/// Reads a back reference at `at` (`\n`, `\nn`, `$n`, `$nn`, `${n}`, `${nn}`), returning the group and its length.
fn backref(replacement: &[u8], at: usize) -> Option<(usize, usize)> {
    let rest = &replacement[at + 1..];
    let braced = replacement[at] == b'$' && rest.first() == Some(&b'{');
    let digits = if braced { &rest[1..] } else { rest };
    let count = digits.iter().take(2).take_while(|b| b.is_ascii_digit()).count();
    if count == 0 {
        return None;
    }
    let group = digits[..count].iter().fold(0, |n, d| n * 10 + usize::from(d - b'0'));
    if braced {
        if digits.get(count) != Some(&b'}') {
            return None;
        }
        return Some((group, count + 3));
    }
    Some((group, count + 1))
}

fn parse_replacement(replacement: &[u8]) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut literal: Vec<u8> = Vec::new();
    let mut last_literal_backslash = false;
    let mut i = 0;
    while i < replacement.len() {
        let b = replacement[i];
        if b == b'\\' || b == b'$' {
            if last_literal_backslash {
                // PHP: a `\` or `$` right after a copied backslash replaces that backslash.
                *literal.last_mut().expect("the backslash") = b;
                last_literal_backslash = false;
                i += 1;
                continue;
            }
            if let Some((group, len)) = backref(replacement, i) {
                if !literal.is_empty() {
                    pieces.push(Piece::Literal(std::mem::take(&mut literal)));
                }
                pieces.push(Piece::Group(group));
                last_literal_backslash = false;
                i += len;
                continue;
            }
        }
        literal.push(b);
        last_literal_backslash = b == b'\\';
        i += 1;
    }
    if !literal.is_empty() {
        pieces.push(Piece::Literal(literal));
    }
    pieces
}

/// PHP's `preg_replace()` on one subject.
///
/// @param pattern
///     The pattern.
/// @param subject
///     The text.
/// @param replacement
///     PHP's replacement syntax: `$n`, `${n}` and `\n` refer to group `n` (up to 99); a group the match didn't set,
///     or that doesn't exist, is empty.
/// @param limit
///     The most replacements to make; `None` is PHP's `-1`.
/// @return
///     The result, borrowed when nothing matched, and how many replacements were made.
pub fn preg_replace<'a>(
    pattern: &Pattern,
    subject: &'a [u8],
    replacement: &[u8],
    limit: Option<usize>,
) -> Result<(Cow<'a, [u8]>, usize), PregError> {
    let pieces = parse_replacement(replacement);
    let mut out = Vec::new();
    let mut copied = 0;
    let count = for_each_match(pattern, subject, limit, |m| {
        out.extend_from_slice(&subject[copied..m.start()]);
        for piece in &pieces {
            match piece {
                Piece::Literal(bytes) => out.extend_from_slice(bytes),
                Piece::Group(n) => out.extend_from_slice(m.get(*n).unwrap_or_default()),
            }
        }
        copied = m.end();
    })?;
    if count == 0 {
        return Ok((Cow::Borrowed(subject), 0));
    }
    out.extend_from_slice(&subject[copied..]);
    Ok((Cow::Owned(out), count))
}

/// PHP's `preg_replace_callback()` on one subject, with no limit.
///
/// @param pattern
///     The pattern.
/// @param subject
///     The text.
/// @param f
///     Gives each match's replacement.
/// @return
///     The result, borrowed when nothing matched.
pub fn preg_replace_callback<'a>(
    pattern: &Pattern,
    subject: &'a [u8],
    mut f: impl FnMut(&Captures<'_>) -> Vec<u8>,
) -> Result<Cow<'a, [u8]>, PregError> {
    let mut out = Vec::new();
    let mut copied = 0;
    let count = for_each_match(pattern, subject, None, |m| {
        out.extend_from_slice(&subject[copied..m.start()]);
        out.extend_from_slice(&f(&m));
        copied = m.end();
    })?;
    if count == 0 {
        return Ok(Cow::Borrowed(subject));
    }
    out.extend_from_slice(&subject[copied..]);
    Ok(Cow::Owned(out))
}

/// `preg_split()`'s flags that WordPress uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SplitFlags {
    /// `PREG_SPLIT_DELIM_CAPTURE`: each match's groups are returned between the pieces.
    pub delim_capture: bool,
    /// `PREG_SPLIT_NO_EMPTY`: empty pieces (and empty captured groups) are left out.
    pub no_empty: bool,
}

/// PHP's `preg_split()`.
///
/// @param pattern
///     The pattern.
/// @param subject
///     The text.
/// @param limit
///     The most pieces; `-1` and `0` mean no limit, as in PHP.
/// @param flags
///     The flags.
/// @return
///     The pieces, borrowed from the subject.
pub fn preg_split<'a>(
    pattern: &Pattern,
    subject: &'a [u8],
    limit: i64,
    flags: SplitFlags,
) -> Result<Vec<&'a [u8]>, PregError> {
    pattern.check(subject)?;
    let mut limit = if limit == 0 { -1 } else { limit };
    let mut pieces = Vec::new();
    let mut locations = pattern.regex.capture_locations();
    let groups = pattern.groups();
    let mut last = 0;
    let mut from = 0;
    while (limit == -1 || limit > 1) && from <= subject.len() {
        if !pattern.next(subject, from, &mut locations)? {
            break;
        }
        let (start, end) = locations.get(0).expect("a match has group 0");
        if start == end {
            if start >= subject.len() {
                break;
            }
            if start == last {
                // An empty match where the last piece ended splits nothing.
                from = pattern.step(subject, start);
                continue;
            }
        }
        if !flags.no_empty || start != last {
            pieces.push(&subject[last..start]);
            if limit != -1 {
                limit -= 1;
            }
        }
        if flags.delim_capture {
            // PHP adds groups up to the last one that is set; an unset group before it is "".
            let highest = (1..groups).rev().find(|&g| locations.get(g).is_some()).unwrap_or(0);
            for g in 1..=highest {
                let piece = locations.get(g).map_or(&subject[0..0], |(s, e)| &subject[s..e]);
                if !flags.no_empty || !piece.is_empty() {
                    pieces.push(piece);
                }
            }
        }
        last = end;
        from = if start == end { pattern.step(subject, end) } else { end };
    }
    if !flags.no_empty || last < subject.len() {
        pieces.push(&subject[last..]);
    }
    Ok(pieces)
}

/// PHP's `preg_match()`: the first match, if any.
///
/// @param pattern
///     The pattern.
/// @param subject
///     The text.
/// @return
///     The first match's groups, `None` when nothing matches.
pub fn preg_match<'a>(pattern: &Pattern, subject: &'a [u8]) -> Result<Option<Captures<'a>>, PregError> {
    pattern.check(subject)?;
    let mut locations = pattern.regex.capture_locations();
    if !pattern.next(subject, 0, &mut locations)? {
        return Ok(None);
    }
    Ok(Some(Captures::read(subject, &locations, pattern.groups())))
}

/// PHP's `preg_match_all()`, keeping one group of every match (`$matches[$group]`).
///
/// @param pattern
///     The pattern.
/// @param subject
///     The text.
/// @param group
///     Which group to keep; an unset group is `""`.
/// @return
///     That group of every match, in order.
pub fn preg_match_all_group<'a>(pattern: &Pattern, subject: &'a [u8], group: usize) -> Result<Vec<&'a [u8]>, PregError> {
    let mut found = Vec::new();
    for_each_match(pattern, subject, None, |m| found.push(m.get(group).unwrap_or_default()))?;
    Ok(found)
}
