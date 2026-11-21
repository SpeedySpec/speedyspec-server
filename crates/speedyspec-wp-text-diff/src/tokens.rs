//! Tokens interned to dense ids, so the engine compares integers instead of bytes.

use std::borrow::Cow;

use foldhash::fast::FixedState;
use hashbrown::HashMap;

use crate::numeric::{Numeric, numeric_equal, parse_numeric};

/// Removes `"\r"` and `"\n"`, as Horde's `Text_Diff::trimNewlines()` does to every token before diffing.
pub fn trim_newlines(token: &[u8]) -> Cow<'_, [u8]> {
    let found = if token.len() < 32 {
        token.iter().any(|&b| b == b'\r' || b == b'\n')
    } else {
        memchr::memchr2(b'\r', b'\n', token).is_some()
    };
    if !found {
        Cow::Borrowed(token)
    } else {
        Cow::Owned(
            token
                .iter()
                .copied()
                .filter(|&b| b != b'\r' && b != b'\n')
                .collect(),
        )
    }
}

/// Both sides of a diff as ids, with what PHP's loose `==` needs to know about each distinct token.
pub struct TokenTable {
    /// The original side's ids.
    pub from: Vec<u32>,
    /// The final side's ids.
    pub to: Vec<u32>,
    numeric: Vec<Option<Numeric>>,
}

impl TokenTable {
    /// Interns both sides' trimmed tokens.
    pub fn new<'a>(from: &[&'a [u8]], to: &[&'a [u8]]) -> TokenTable {
        let mut ids: HashMap<Cow<'a, [u8]>, u32, FixedState> =
            HashMap::with_capacity_and_hasher(from.len() + to.len(), FixedState::default());
        let mut numeric = Vec::new();
        let mut intern = |token: &'a [u8]| -> u32 {
            let trimmed = trim_newlines(token);
            if let Some(&id) = ids.get(trimmed.as_ref()) {
                return id;
            }
            let id = numeric.len() as u32;
            numeric.push(maybe_numeric(&trimmed));
            ids.insert(trimmed, id);
            id
        };
        let from: Vec<u32> = from.iter().map(|t| intern(t)).collect();
        let to: Vec<u32> = to.iter().map(|t| intern(t)).collect();
        TokenTable { from, to, numeric }
    }

    /// How many distinct tokens there are; ids are below this.
    pub fn distinct(&self) -> usize {
        self.numeric.len()
    }

    /// PHP 8's `==` between the tokens with ids `a` and `b`.
    #[inline]
    pub fn loose_eq(&self, a: u32, b: u32) -> bool {
        a == b || numeric_equal(self.numeric[a as usize], self.numeric[b as usize])
    }
}

fn maybe_numeric(token: &[u8]) -> Option<Numeric> {
    match token.first() {
        Some(b)
            if b.is_ascii_digit()
                || matches!(
                    b,
                    b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C | b'+' | b'-' | b'.'
                ) =>
        {
            parse_numeric(token)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_trimmed_token_same_id() {
        let t = TokenTable::new(&[b"a\r", b"b"], &[b"a", b"c\n", b"b"]);
        assert_eq!(t.from, [0, 1]);
        assert_eq!(t.to, [0, 2, 1]);
        assert_eq!(t.distinct(), 3);
    }

    #[test]
    fn numeric_tokens_are_loosely_equal() {
        let t = TokenTable::new(&[b"1e3"], &[b"1000", b"x"]);
        assert!(t.loose_eq(t.from[0], t.to[0]));
        assert!(!t.loose_eq(t.from[0], t.to[1]));
    }
}
