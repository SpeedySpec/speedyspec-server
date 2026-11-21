//! The HTML5 named character references, looked up as `WP_Token_Map::read_token()` does: case-sensitively, the
//! longest name that starts the text.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::named::NAMED;

/// `WP_Token_Map`'s key length: names longer than this are grouped by their first two bytes.
const KEY_LENGTH: usize = 2;

/// A name and its UTF-8 replacement.
type Entry = (&'static [u8], &'static [u8]);

struct Index {
    /// Names longer than the key, by their first two bytes, longest first then alphabetical.
    long: HashMap<[u8; KEY_LENGTH], Vec<Entry>>,
    /// Names of at most two bytes, longest first.
    small: Vec<Entry>,
}

fn index() -> &'static Index {
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut long: HashMap<[u8; KEY_LENGTH], Vec<Entry>> = HashMap::new();
        let mut small = Vec::new();
        for &(name, value) in NAMED {
            if name.len() > KEY_LENGTH {
                long.entry([name[0], name[1]])
                    .or_default()
                    .push((name, value));
            } else {
                small.push((name, value));
            }
        }
        for group in long.values_mut() {
            group.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(b.0)));
        }
        small.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(b.0)));
        Index { long, small }
    })
}

/// How many names the table holds.
pub fn named_reference_count() -> usize {
    NAMED.len()
}

/// The replacement for the longest named reference starting at `offset`, and the name's length.
///
/// As `read_token()`: when the whole text is longer than two bytes, a NUL in the first two bytes at `offset` stops
/// the search, and long names are tried before short ones.
pub fn named_reference(text: &[u8], offset: usize) -> Option<(&'static [u8], usize)> {
    let rest = text.get(offset..)?;
    let index = index();
    if text.len() > KEY_LENGTH {
        if rest.iter().take(KEY_LENGTH).any(|b| *b == 0) {
            return None;
        }
        let group = (rest.len() >= KEY_LENGTH)
            .then(|| index.long.get(&[rest[0], rest[1]]))
            .flatten();
        if let Some((name, value)) =
            group.and_then(|g| g.iter().find(|(name, _)| rest.starts_with(name)))
        {
            return Some((value, name.len()));
        }
    }
    let first = *rest.first()?;
    index
        .small
        .iter()
        .find(|(name, _)| name[0] == first && rest.starts_with(name))
        .map(|(name, value)| (*value, name.len()))
}
