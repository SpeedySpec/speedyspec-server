use speedyspec_wp_shortcode_contracts::{AttsParse, AttsParser};

use super::shared::{Found, is_space, parse_with};

/// `shortcode_parse_atts()` as a byte state machine.
///
/// At each position it tries the pattern's alternatives in order. None of them can gain from backtracking (each
/// greedy run is followed by a byte the run excludes), so one forward pass per alternative is exact.
#[derive(Default)]
pub struct HandAttsParser;

impl HandAttsParser {
    /// A parser.
    pub fn new() -> Self {
        Self
    }
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// `(?:\s|$)` at `i`: where the match ends, or `None`.
fn tail(t: &[u8], i: usize) -> Option<usize> {
    match t.get(i) {
        None => Some(i),
        Some(&b) if is_space(b) => Some(i + 1),
        Some(_) => None,
    }
}

fn run(t: &[u8], mut i: usize, keep: impl Fn(u8) -> bool) -> usize {
    while i < t.len() && keep(t[i]) {
        i += 1;
    }
    i
}

/// `"…"` or `'…'` starting at `i`: the inside and where the match ends.
fn quoted(t: &[u8], i: usize, q: u8) -> Option<(&[u8], usize)> {
    if t.get(i) != Some(&q) {
        return None;
    }
    let close = i + 1 + t[i + 1..].iter().position(|&b| b == q)?;
    Some((&t[i + 1..close], tail(t, close + 1)?))
}

fn named(t: &[u8], p: usize) -> Option<(Found<'_>, usize)> {
    let name_end = run(t, p, is_word);
    if name_end == p {
        return None;
    }
    let eq = run(t, name_end, is_space);
    if t.get(eq) != Some(&b'=') {
        return None;
    }
    let v = run(t, eq + 1, is_space);
    let name = &t[p..name_end];
    if let Some((inner, end)) = quoted(t, v, b'"').or_else(|| quoted(t, v, b'\'')) {
        return Some((Found::Named(name, inner), end));
    }
    let v_end = run(t, v, |b| !is_space(b) && b != b'\'' && b != b'"');
    if v_end == v {
        return None;
    }
    Some((Found::Named(name, &t[v..v_end]), tail(t, v_end)?))
}

fn at(t: &[u8], p: usize) -> Option<(Found<'_>, usize)> {
    if let Some(found) = named(t, p) {
        return Some(found);
    }
    for q in *b"\"'" {
        if let Some((inner, end)) = quoted(t, p, q) {
            return Some((Found::Quoted(inner), end));
        }
    }
    let end = run(t, p, |b| !is_space(b));
    (end > p).then(|| (Found::Bare(&t[p..end]), tail(t, end).expect("a run ends at a space or the end")))
}

impl AttsParser for HandAttsParser {
    fn parse(&self, text: &[u8]) -> AttsParse {
        parse_with(text, |t| {
            let mut found = Vec::new();
            let mut p = 0;
            while p < t.len() {
                match at(t, p) {
                    Some((f, end)) => {
                        found.push(f);
                        p = end;
                    }
                    None => p += 1,
                }
            }
            found
        })
    }
}
