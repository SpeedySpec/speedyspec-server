//! `htmlentities2()`.

use std::sync::LazyLock;

use aho_corasick::{AhoCorasick, MatchKind};

use crate::html401::HTML401_TABLE;

/// The characters PHP's `HTML_ENTITIES` table translates, except `&`, which `htmlentities2()` keeps.
static TABLE: LazyLock<(AhoCorasick, Vec<&'static str>)> = LazyLock::new(|| {
    let entries: Vec<(&[u8], &str)> = HTML401_TABLE
        .iter()
        .copied()
        .filter(|(character, _)| *character != b"&")
        .collect();
    let automaton = AhoCorasick::builder()
        .match_kind(MatchKind::LeftmostLongest)
        .build(entries.iter().map(|(character, _)| *character))
        .expect("the entity table builds");
    (automaton, entries.iter().map(|(_, entity)| *entity).collect())
});

/// `htmlentities2()`: every character with an HTML 4.01 entity becomes that entity, and every `&` that does not
/// already start something entity-shaped becomes `&amp;`.
pub fn htmlentities2(text: &[u8]) -> Vec<u8> {
    let (automaton, entities) = &*TABLE;
    let mut translated = Vec::with_capacity(text.len() + text.len() / 4);
    automaton.replace_all_with_bytes(text, &mut translated, |m, _, out| {
        out.extend_from_slice(entities[m.pattern().as_usize()].as_bytes());
        true
    });
    let mut out = Vec::with_capacity(translated.len() + 16);
    for (i, &b) in translated.iter().enumerate() {
        if b == b'&' && !entity_shaped(&translated[i + 1..]) {
            out.extend_from_slice(b"&amp;");
        } else {
            out.push(b);
        }
    }
    out
}

/// `[A-Za-z]{0,4}\w{2,3};|#[0-9]{2,3};` at the start of `s` (the text after an `&`), with the regex's backtracking.
fn entity_shaped(s: &[u8]) -> bool {
    let word = |b: &u8| b.is_ascii_alphanumeric() || *b == b'_';
    let letters = s.iter().take(4).take_while(|b| b.is_ascii_alphabetic()).count();
    let named = (0..=letters).any(|a| {
        (2..=3).any(|b| {
            s.len() > a + b && s[a..a + b].iter().all(word) && s[a + b] == b';'
        })
    });
    let numeric = s.first() == Some(&b'#')
        && (2..=3).any(|n| {
            s.len() > n + 1 && s[1..1 + n].iter().all(u8::is_ascii_digit) && s[1 + n] == b';'
        });
    named || numeric
}
