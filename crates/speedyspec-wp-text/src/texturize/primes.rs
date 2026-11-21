//! `wptexturize_primes()`: tells primes (9′) from closing quotes.

use memchr::memmem;

use crate::preg::{Pattern, PregError, preg_replace};
use crate::strings::replace_one;

const FLAG: &[u8] = b"<!--wp-prime-or-quote-->";

/// The four patterns `wptexturize_primes()` builds for one needle and spaces fragment.
pub(crate) struct PrimePatterns {
    needle: u8,
    quote: Pattern,
    prime: Pattern,
    flag_after_digit: Pattern,
    flag_no_digit: Pattern,
}

impl PrimePatterns {
    pub(crate) fn new(needle: u8, spaces: &[u8]) -> Result<PrimePatterns, PregError> {
        let n = needle as char;
        let mut quote = format!(r"/{n}(?=\Z|[.,:;!?)}}\-\]]|&gt;|").into_bytes();
        quote.extend_from_slice(spaces);
        quote.extend_from_slice(b")/");
        Ok(PrimePatterns {
            needle,
            quote: Pattern::try_php(&quote)?,
            prime: Pattern::php(&format!(r"/(?<=\d){n}/")),
            flag_after_digit: Pattern::php(r"/(?<=\d)<!--wp-prime-or-quote-->/"),
            flag_no_digit: Pattern::php(r"/(?<!\d)<!--wp-prime-or-quote-->/"),
        })
    }
}

fn replace(pattern: &Pattern, text: Vec<u8>, replacement: &[u8]) -> Result<(Vec<u8>, usize), PregError> {
    let (out, count) = preg_replace(pattern, &text, replacement, None)?;
    Ok((if count == 0 { text } else { out.into_owned() }, count))
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() { 0 } else { memmem::find_iter(haystack, needle).count() }
}

/// `wptexturize_primes( $haystack, $needle, $prime, $open_quote, $close_quote )`.
pub(crate) fn wptexturize_primes(
    haystack: &[u8],
    patterns: &PrimePatterns,
    prime: &[u8],
    open_quote: &[u8],
    close_quote: &[u8],
) -> Result<Vec<u8>, PregError> {
    let needle = [patterns.needle];
    let mut sentences: Vec<Vec<u8>> = Vec::new();
    let mut rest = haystack;
    while let Some(at) = memmem::find(rest, open_quote) {
        sentences.push(rest[..at].to_vec());
        rest = &rest[at + open_quote.len()..];
    }
    sentences.push(rest.to_vec());
    for (key, sentence) in sentences.iter_mut().enumerate() {
        if memchr::memchr(patterns.needle, sentence).is_none() {
            continue;
        }
        let mut s = std::mem::take(sentence);
        if key != 0 && count(&s, close_quote) == 0 {
            let (flagged, found) = replace(&patterns.quote, s, FLAG)?;
            s = flagged;
            if found > 1 {
                let (closed, closed_count) = replace(&patterns.flag_no_digit, s, close_quote)?;
                s = closed;
                if closed_count == 0 {
                    let flag_period = [FLAG, b"."].concat();
                    let pos = memmem::rfind(&s, &flag_period).or_else(|| memmem::rfind(&s, FLAG)).unwrap_or(0);
                    s.splice(pos..(pos + FLAG.len()).min(s.len()), close_quote.iter().copied());
                }
                s = replace(&patterns.prime, s, prime)?.0;
                s = replace(&patterns.flag_after_digit, s, prime)?.0;
                s = replace_one(&s, FLAG, close_quote).into_owned();
            } else if found == 1 {
                s = replace_one(&s, FLAG, close_quote).into_owned();
                s = replace(&patterns.prime, s, prime)?.0;
            } else {
                s = replace(&patterns.prime, s, prime)?.0;
            }
        } else {
            s = replace(&patterns.prime, s, prime)?.0;
            s = replace(&patterns.quote, s, close_quote)?.0;
        }
        if needle == *b"\"" && memchr::memchr(b'"', &s).is_some() {
            s = replace_one(&s, b"\"", close_quote).into_owned();
        }
        *sentence = s;
    }
    Ok(sentences.join(open_quote))
}
