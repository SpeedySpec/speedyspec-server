//! `wptexturize()` with hand-written scanners in place of WordPress's patterns, for the default spaces fragment.
//!
//! Each scanner below reproduces one of WordPress's patterns over `[\r\n\t ]|\xC2\xA0|&nbsp;` exactly: the same
//! matches, left to right, non-overlapping, with the same lookarounds, and the passes run in WordPress's order.
//! The PCRE2 transcription ([`PcreTexturizer`]) is the oracle; `tests/differential.rs` compares the two.

use std::borrow::Cow;

use memchr::{memchr, memchr2};
use speedyspec_wp_html_split::{Escapes, tag_end};
use speedyspec_wp_shortcode_contracts::TagName;

use crate::preg::{PregError, preg_replace};
use crate::scan::{pass, then};
use crate::strings::{PHP_TRIM_DEFAULT, php_trim, replace_one};

use super::pcre::{PcreTexturizer, TIMES};
use super::tables::{APOS_FLAG, OPEN_Q_FLAG, OPEN_SQ_FLAG};
use super::{TexturizeScope, TexturizeTables, Texturizer, pushpop_element};

const PRIME_FLAG: &[u8] = b"<!--wp-prime-or-quote-->";

/// `(?=SP)`: a space token starts at `p`.
fn space_at(t: &[u8], p: usize) -> bool {
    matches!(t.get(p), Some(b'\r' | b'\n' | b'\t' | b' '))
        || t.get(p..).is_some_and(|r| r.starts_with(b"\xC2\xA0") || r.starts_with(b"&nbsp;"))
}

/// `(?<=SP)`: a space token ends at `p`.
fn space_before(t: &[u8], p: usize) -> bool {
    let before = &t[..p];
    matches!(before.last(), Some(b'\r' | b'\n' | b'\t' | b' ')) || before.ends_with(b"\xC2\xA0") || before.ends_with(b"&nbsp;")
}

/// `\Z` and `$` (no `D`, no `m`): the end, or a newline that is the last byte.
fn at_end(t: &[u8], p: usize) -> bool {
    p == t.len() || (p + 1 == t.len() && t[p] == b'\n')
}

fn digit(t: &[u8], p: usize) -> bool {
    t.get(p).is_some_and(u8::is_ascii_digit)
}

/// `(?=\Z|[.,:;!?)}\-\]]|&gt;|SP)`: what may follow a closing quote.
fn closes(t: &[u8], p: usize) -> bool {
    at_end(t, p)
        || matches!(t.get(p), Some(b'.' | b',' | b':' | b';' | b'!' | b'?' | b')' | b'}' | b'-' | b']'))
        || t.get(p..).is_some_and(|r| r.starts_with(b"&gt;"))
        || space_at(t, p)
}

/// Replaces each `&` with `&#038;` unless it already looks like an entity
/// (`/&(?!#(?:\d+|x[a-f0-9]+);|[a-z1-4]{1,8};)/i`).
pub(crate) fn ampersands(t: &[u8]) -> Cow<'_, [u8]> {
    pass(t, b'&', |t, i, out| {
        let r = &t[i + 1..];
        let run = |from: usize, class: fn(&u8) -> bool| r.get(from..).map_or(0, |s| s.iter().take_while(|b| class(b)).count());
        let entity = if r.first() == Some(&b'#') {
            let digits = run(1, u8::is_ascii_digit);
            let numeric = digits > 0 && r.get(1 + digits) == Some(&b';');
            let hex = matches!(r.get(1), Some(b'x' | b'X')) && {
                let n = run(2, u8::is_ascii_hexdigit);
                n > 0 && r.get(2 + n) == Some(&b';')
            };
            numeric || hex
        } else {
            let n = run(0, |b| b.is_ascii_alphabetic() || (b'1'..=b'4').contains(b));
            (1..=8).contains(&n) && r.get(n) == Some(&b';')
        };
        if entity {
            return None;
        }
        out.extend_from_slice(b"&#038;");
        Some(i + 1)
    })
}

/// The apostrophe patterns, in WordPress's order (all present: [`TexturizeTables::hand_compatible`]).
fn apostrophes<'a>(text: &'a [u8], tables: &TexturizeTables) -> Cow<'a, [u8]> {
    // '(\d\d)'(?=closes) and '(\d\d)"(?=closes)
    let text = pass(text, b'\'', |t, i, out| {
        if !(digit(t, i + 1) && digit(t, i + 2) && t.get(i + 3) == Some(&b'\'') && closes(t, i + 4)) {
            return None;
        }
        out.extend_from_slice(APOS_FLAG);
        out.extend_from_slice(&t[i + 1..i + 3]);
        out.extend_from_slice(&tables.closing_single_quote);
        Some(i + 4)
    });
    let text = then(text, |s| {
        pass(s, b'\'', |t, i, out| {
            if !(digit(t, i + 1) && digit(t, i + 2) && t.get(i + 3) == Some(&b'"') && closes(t, i + 4)) {
                return None;
            }
            out.extend_from_slice(APOS_FLAG);
            out.extend_from_slice(&t[i + 1..i + 3]);
            out.extend_from_slice(&tables.closing_quote);
            Some(i + 4)
        })
    });
    // '(?=\d\d(?:\Z|(?![%\d]|[.,]\d)))
    let text = then(text, |s| {
        pass(s, b'\'', |t, i, out| {
            if !(digit(t, i + 1) && digit(t, i + 2)) {
                return None;
            }
            let q = i + 3;
            let excluded = matches!(t.get(q), Some(b'%')) || digit(t, q) || (matches!(t.get(q), Some(b'.' | b',')) && digit(t, q + 1));
            if !(at_end(t, q) || !excluded) {
                return None;
            }
            out.extend_from_slice(APOS_FLAG);
            Some(i + 1)
        })
    });
    // (?<=\A|SP)'(\d[.,\d]*)'
    let text = then(text, |s| {
        pass(s, b'\'', |t, i, out| {
            if !((i == 0 || space_before(t, i)) && digit(t, i + 1)) {
                return None;
            }
            let mut j = i + 2;
            while matches!(t.get(j), Some(b'.' | b',' | b'0'..=b'9')) {
                j += 1;
            }
            if t.get(j) != Some(&b'\'') {
                return None;
            }
            out.extend_from_slice(OPEN_SQ_FLAG);
            out.extend_from_slice(&t[i + 1..j]);
            out.extend_from_slice(&tables.closing_single_quote);
            Some(j + 1)
        })
    });
    // (?<=\A|[([{"\-]|&lt;|SP)'
    let text = then(text, |s| {
        pass(s, b'\'', |t, i, out| {
            let opens = i == 0
                || matches!(t[i - 1], b'(' | b'[' | b'{' | b'"' | b'-')
                || t[..i].ends_with(b"&lt;")
                || space_before(t, i);
            if !opens {
                return None;
            }
            out.extend_from_slice(OPEN_SQ_FLAG);
            Some(i + 1)
        })
    });
    // (?<!SP)'(?!\Z|[.,:;!?"'(){}[\]\-]|&[lg]t;|SP)
    then(text, |s| {
        pass(s, b'\'', |t, i, out| {
            if space_before(t, i) {
                return None;
            }
            let p = i + 1;
            let stops = at_end(t, p)
                || matches!(
                    t.get(p),
                    Some(b'.' | b',' | b':' | b';' | b'!' | b'?' | b'"' | b'\'' | b'(' | b')' | b'{' | b'}' | b'[' | b']' | b'-')
                )
                || t.get(p..).is_some_and(|r| r.starts_with(b"&lt;") || r.starts_with(b"&gt;"))
                || space_at(t, p);
            if stops {
                return None;
            }
            out.extend_from_slice(APOS_FLAG);
            Some(i + 1)
        })
    })
}

/// The double-quote patterns, in WordPress's order.
fn double_quotes<'a>(text: &'a [u8], tables: &TexturizeTables) -> Cow<'a, [u8]> {
    // (?<=\A|SP)"(\d[.,\d]*)"
    let text = pass(text, b'"', |t, i, out| {
        if !((i == 0 || space_before(t, i)) && digit(t, i + 1)) {
            return None;
        }
        let mut j = i + 2;
        while matches!(t.get(j), Some(b'.' | b',' | b'0'..=b'9')) {
            j += 1;
        }
        if t.get(j) != Some(&b'"') {
            return None;
        }
        out.extend_from_slice(OPEN_Q_FLAG);
        out.extend_from_slice(&t[i + 1..j]);
        out.extend_from_slice(&tables.closing_quote);
        Some(j + 1)
    });
    // (?<=\A|[([{\-]|&lt;|SP)"(?!SP)
    then(text, |s| {
        pass(s, b'"', |t, i, out| {
            let opens = i == 0 || matches!(t[i - 1], b'(' | b'[' | b'{' | b'-') || t[..i].ends_with(b"&lt;") || space_before(t, i);
            if !opens || space_at(t, i + 1) {
                return None;
            }
            out.extend_from_slice(OPEN_Q_FLAG);
            Some(i + 1)
        })
    })
}

/// The dash patterns, in WordPress's order.
fn dashes<'a>(text: &'a [u8], tables: &TexturizeTables) -> Cow<'a, [u8]> {
    let text = pass(text, b'-', |t, i, out| {
        if !t[i..].starts_with(b"---") {
            return None;
        }
        out.extend_from_slice(&tables.em_dash);
        Some(i + 3)
    });
    let text = then(text, |s| {
        pass(s, b'-', |t, i, out| {
            if !((i == 0 || space_before(t, i)) && t.get(i + 1) == Some(&b'-') && (at_end(t, i + 2) || space_at(t, i + 2))) {
                return None;
            }
            out.extend_from_slice(&tables.em_dash);
            Some(i + 2)
        })
    });
    let text = then(text, |s| {
        pass(s, b'-', |t, i, out| {
            if t.get(i + 1) != Some(&b'-') || (i >= 2 && &t[i - 2..i] == b"xn") {
                return None;
            }
            out.extend_from_slice(&tables.en_dash);
            Some(i + 2)
        })
    });
    then(text, |s| {
        pass(s, b'-', |t, i, out| {
            if !((i == 0 || space_before(t, i)) && (at_end(t, i + 1) || space_at(t, i + 1))) {
                return None;
            }
            out.extend_from_slice(&tables.en_dash);
            Some(i + 1)
        })
    })
}

/// A pass over the occurrences of the prime-or-quote flag, keeping those `keep` accepts.
fn flags<'a>(text: &'a [u8], replacement: &[u8], keep: impl Fn(&[u8], usize) -> bool) -> (Cow<'a, [u8]>, usize) {
    let mut count = 0;
    let out = pass(text, b'<', |t, i, out| {
        if !t[i..].starts_with(PRIME_FLAG) || !keep(t, i) {
            return None;
        }
        count += 1;
        out.extend_from_slice(replacement);
        Some(i + PRIME_FLAG.len())
    });
    (out, count)
}

/// `wptexturize_primes()` for the default spaces fragment.
fn primes(haystack: &[u8], needle: u8, prime: &[u8], open_quote: &[u8], close_quote: &[u8]) -> Vec<u8> {
    let prime_pass = |s: &[u8]| -> Vec<u8> {
        pass(s, needle, |t, i, out| {
            if !(i >= 1 && t[i - 1].is_ascii_digit()) {
                return None;
            }
            out.extend_from_slice(prime);
            Some(i + 1)
        })
        .into_owned()
    };
    let quote_pass = |s: &[u8], replacement: &[u8], count: &mut usize| -> Vec<u8> {
        pass(s, needle, |t, i, out| {
            if !closes(t, i + 1) {
                return None;
            }
            *count += 1;
            out.extend_from_slice(replacement);
            Some(i + 1)
        })
        .into_owned()
    };
    let mut out = Vec::with_capacity(haystack.len() + 16);
    let mut rest = haystack;
    let mut key = 0;
    loop {
        let (sentence, next) = match memchr::memmem::find(rest, open_quote) {
            Some(at) => (&rest[..at], Some(&rest[at + open_quote.len()..])),
            None => (rest, None),
        };
        if key > 0 {
            out.extend_from_slice(open_quote);
        }
        if memchr(needle, sentence).is_none() {
            out.extend_from_slice(sentence);
        } else {
            let closing = !close_quote.is_empty() && memchr::memmem::find(sentence, close_quote).is_some();
            let mut s: Vec<u8>;
            if key != 0 && !closing {
                let mut found = 0;
                s = quote_pass(sentence, PRIME_FLAG, &mut found);
                if found > 1 {
                    let (closed, closed_count) = flags(&s, close_quote, |t, i| !(i >= 1 && t[i - 1].is_ascii_digit()));
                    s = closed.into_owned();
                    if closed_count == 0 {
                        let flag_period = [PRIME_FLAG, b"."].concat();
                        let pos = memchr::memmem::rfind(&s, &flag_period).or_else(|| memchr::memmem::rfind(&s, PRIME_FLAG)).unwrap_or(0);
                        s.splice(pos..(pos + PRIME_FLAG.len()).min(s.len()), close_quote.iter().copied());
                    }
                    s = prime_pass(&s);
                    s = flags(&s, prime, |t, i| i >= 1 && t[i - 1].is_ascii_digit()).0.into_owned();
                    s = replace_one(&s, PRIME_FLAG, close_quote).into_owned();
                } else if found == 1 {
                    s = replace_one(&s, PRIME_FLAG, close_quote).into_owned();
                    s = prime_pass(&s);
                } else {
                    s = prime_pass(&s);
                }
            } else {
                s = prime_pass(sentence);
                s = quote_pass(&s, close_quote, &mut 0);
            }
            if needle == b'"' && memchr(b'"', &s).is_some() {
                s = replace_one(&s, b"\"", close_quote).into_owned();
            }
            out.extend_from_slice(&s);
        }
        key += 1;
        match next {
            Some(n) => rest = n,
            None => break,
        }
    }
    out
}

/// Texturizes one text chunk that is neither a tag nor inside a no-texturize element.
fn texturize_chunk<'a>(chunk: &'a [u8], tables: &TexturizeTables) -> Result<Cow<'a, [u8]>, PregError> {
    let mut curl = tables.static_replace.apply(chunk);
    if memchr(b'\'', &curl).is_some() {
        let quoted = apostrophes(&curl, tables).into_owned();
        let primed = primes(&quoted, b'\'', &tables.prime, OPEN_SQ_FLAG, &tables.closing_single_quote);
        let primed = replace_one(&primed, APOS_FLAG, &tables.apos).into_owned();
        curl = Cow::Owned(replace_one(&primed, OPEN_SQ_FLAG, &tables.opening_single_quote).into_owned());
    }
    if memchr(b'"', &curl).is_some() {
        let quoted = double_quotes(&curl, tables).into_owned();
        let primed = primes(&quoted, b'"', &tables.double_prime, OPEN_Q_FLAG, &tables.closing_quote);
        curl = Cow::Owned(replace_one(&primed, OPEN_Q_FLAG, &tables.opening_quote).into_owned());
    }
    if memchr(b'-', &curl).is_some() {
        curl = then(curl, |s| dashes(s, tables));
    }
    // 9x9 (times), but never 0x9999: rare, so WordPress's own pattern does the replacing.
    let has_times = {
        let t = &curl[..];
        let mut from = 0;
        let mut found = false;
        while let Some(at) = memchr(b'x', &t[from..]) {
            let i = from + at;
            if i >= 1 && t[i - 1].is_ascii_digit() && digit(t, i + 1) {
                found = true;
                break;
            }
            from = i + 1;
        }
        found
    };
    if has_times {
        let (out, count) = preg_replace(&TIMES, &curl, b"$1&#215;$2", None)?;
        if count > 0 {
            curl = Cow::Owned(out.into_owned());
        }
    }
    Ok(then(curl, ampersands))
}

/// The registered names among those the text mentions (`@\[/?([^<>&/\[\]\x00-\x20=]++)@`), in registration order.
fn found_tags(text: &[u8], registered: &[TagName]) -> Vec<TagName> {
    if registered.is_empty() {
        return Vec::new();
    }
    let mut mentioned: Vec<&[u8]> = Vec::new();
    let mut from = 0;
    while let Some(at) = memchr(b'[', &text[from..]) {
        let mut j = from + at + 1;
        if text.get(j) == Some(&b'/') {
            j += 1;
        }
        let start = j;
        while text.get(j).is_some_and(|&b| b > 0x20 && !matches!(b, b'<' | b'>' | b'&' | b'/' | b'[' | b']' | b'=')) {
            j += 1;
        }
        if j > start {
            mentioned.push(&text[start..j]);
        }
        from = (from + at + 1).max(j);
    }
    registered.iter().filter(|t| mentioned.contains(&t.as_bytes())).cloned().collect()
}

/// Where the shortcode that `_get_wptexturize_shortcode_regex()` would match at `k` ends, if it matches.
fn shortcode_end(t: &[u8], k: usize, found: &[TagName]) -> Option<usize> {
    let mut j = k + 1;
    if matches!(t.get(j), Some(b'/' | b'[')) {
        j += 1;
    }
    let delimiter = |b: Option<&u8>| matches!(b, Some(b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b']' | b'/'));
    let name = found.iter().find(|n| t[j..].starts_with(n.as_bytes()) && delimiter(t.get(j + n.as_bytes().len())))?;
    j += name.as_bytes().len();
    loop {
        match t.get(j) {
            Some(b'[' | b']' | b'>') | None => break,
            Some(b'<') => {
                let mut m = j + 1;
                while t.get(m).is_some_and(|b| !matches!(b, b'[' | b']' | b'>')) {
                    m += 1;
                }
                if t.get(m) == Some(&b'>') {
                    j = m + 1;
                } else {
                    break;
                }
            }
            Some(_) => {
                while t.get(j).is_some_and(|b| !matches!(b, b'[' | b']' | b'<' | b'>')) {
                    j += 1;
                }
            }
        }
    }
    if t.get(j) != Some(&b']') {
        return None;
    }
    j += 1;
    if t.get(j) == Some(&b']') {
        j += 1;
    }
    Some(j)
}

/// `wptexturize()` with hand-written scanners; tables it can't run go to [`PcreTexturizer`].
#[derive(Default)]
pub struct HandTexturizer {
    fallback: PcreTexturizer,
}

impl HandTexturizer {
    fn text_chunk(
        &self,
        chunk: &[u8],
        tables: &TexturizeTables,
        stacks_empty: bool,
        out: &mut Vec<u8>,
    ) -> Result<(), PregError> {
        if chunk.is_empty() {
            return Ok(());
        }
        if !stacks_empty || php_trim(chunk, PHP_TRIM_DEFAULT).is_empty() {
            out.extend_from_slice(chunk);
            return Ok(());
        }
        out.extend_from_slice(&texturize_chunk(chunk, tables)?);
        Ok(())
    }
}

impl Texturizer for HandTexturizer {
    fn texturize<'a>(
        &self,
        text: &'a [u8],
        tables: &TexturizeTables,
        scope: &TexturizeScope<'_>,
    ) -> Result<Cow<'a, [u8]>, PregError> {
        if !tables.hand_compatible() {
            return self.fallback.texturize(text, tables, scope);
        }
        if text.is_empty() || text == b"0" {
            return Ok(Cow::Borrowed(text));
        }
        let found = if memchr(b'[', text).is_some() { found_tags(text, scope.shortcode_tags) } else { Vec::new() };
        let mut tags_stack: Vec<Vec<u8>> = Vec::new();
        let mut shortcodes_stack: Vec<Vec<u8>> = Vec::new();
        let mut out = Vec::with_capacity(text.len() + text.len() / 8);
        let mut start = 0;
        let mut scan = 0;
        loop {
            let next = if found.is_empty() { memchr(b'<', &text[scan..]) } else { memchr2(b'<', b'[', &text[scan..]) };
            let Some(at) = next else { break };
            let k = scan + at;
            if text[k] == b'<' {
                let end = tag_end(text, k, Escapes::CommentsOnly);
                self.text_chunk(&text[start..k], tables, tags_stack.is_empty() && shortcodes_stack.is_empty(), &mut out)?;
                let element = &text[k..end];
                if element.starts_with(b"<!--") {
                    out.extend_from_slice(element);
                } else {
                    let element = ampersands(element);
                    pushpop_element(&element, &mut tags_stack, scope.no_texturize_tags);
                    out.extend_from_slice(&element);
                }
                start = end;
                scan = end;
            } else if let Some(end) = shortcode_end(text, k, &found) {
                self.text_chunk(&text[start..k], tables, tags_stack.is_empty() && shortcodes_stack.is_empty(), &mut out)?;
                let shortcode = &text[k..end];
                if !shortcode.starts_with(b"[[") && !shortcode.ends_with(b"]]") {
                    pushpop_element(shortcode, &mut shortcodes_stack, scope.no_texturize_shortcodes);
                }
                out.extend_from_slice(shortcode);
                start = end;
                scan = end;
            } else {
                scan = k + 1;
            }
            if scan >= text.len() {
                break;
            }
        }
        self.text_chunk(&text[start..], tables, tags_stack.is_empty() && shortcodes_stack.is_empty(), &mut out)?;
        if out == text {
            return Ok(Cow::Borrowed(text));
        }
        Ok(Cow::Owned(out))
    }
}
