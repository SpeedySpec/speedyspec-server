//! Building blocks for the hand-written passes that replace WordPress's patterns on the fast paths.

use std::borrow::Cow;

use memchr::memchr;
use memchr::memmem::Finder;

/// One pattern's pass: at each `trigger` byte, left to right, `matcher` either writes the replacement into its
/// buffer and returns where the match ends (past `i`), or returns `None`. Matches don't overlap, as with
/// `preg_replace()`; unchanged text is borrowed.
pub(crate) fn pass<'a>(
    subject: &'a [u8],
    trigger: u8,
    matcher: impl FnMut(&[u8], usize, &mut Vec<u8>) -> Option<usize>,
) -> Cow<'a, [u8]> {
    run(subject, |s, from| memchr(trigger, &s[from..]).map(|at| from + at), matcher)
}

/// [`pass`], with candidates where `needle` occurs.
pub(crate) fn pass_needle<'a>(
    subject: &'a [u8],
    needle: &[u8],
    matcher: impl FnMut(&[u8], usize, &mut Vec<u8>) -> Option<usize>,
) -> Cow<'a, [u8]> {
    let finder = Finder::new(needle);
    run(subject, |s, from| finder.find(&s[from..]).map(|at| from + at), matcher)
}

fn run<'a>(
    subject: &'a [u8],
    next: impl Fn(&[u8], usize) -> Option<usize>,
    mut matcher: impl FnMut(&[u8], usize, &mut Vec<u8>) -> Option<usize>,
) -> Cow<'a, [u8]> {
    let mut out: Option<Vec<u8>> = None;
    let mut copied = 0;
    let mut from = 0;
    let mut replacement = Vec::new();
    while from < subject.len() {
        let Some(i) = next(subject, from) else { break };
        replacement.clear();
        match matcher(subject, i, &mut replacement) {
            Some(end) => {
                let buffer = out.get_or_insert_with(|| Vec::with_capacity(subject.len() + 32));
                buffer.extend_from_slice(&subject[copied..i]);
                buffer.extend_from_slice(&replacement);
                copied = end;
                from = end.max(i + 1);
            }
            None => from = i + 1,
        }
    }
    match out {
        None => Cow::Borrowed(subject),
        Some(mut buffer) => {
            buffer.extend_from_slice(&subject[copied..]);
            Cow::Owned(buffer)
        }
    }
}

/// Runs a pass on an owned or borrowed text, keeping it owned only when it changed.
pub(crate) fn then<'a>(text: Cow<'a, [u8]>, run: impl FnOnce(&[u8]) -> Cow<'_, [u8]>) -> Cow<'a, [u8]> {
    match text {
        Cow::Borrowed(b) => run(b),
        Cow::Owned(o) => match run(&o) {
            Cow::Borrowed(_) => Cow::Owned(o),
            Cow::Owned(changed) => Cow::Owned(changed),
        },
    }
}

/// PCRE's `\s` without UCP: space, tab, newline, vertical tab, form feed, carriage return.
pub(crate) fn is_pcre_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// Where a run of `\s` starting at `p` ends.
pub(crate) fn skip_spaces(t: &[u8], mut p: usize) -> usize {
    while t.get(p).is_some_and(|&b| is_pcre_space(b)) {
        p += 1;
    }
    p
}
