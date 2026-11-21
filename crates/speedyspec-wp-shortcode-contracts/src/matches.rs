use std::ops::Range;

/// One match of `get_shortcode_regex()`, as spans into the haystack.
///
/// Group 0 is the whole match. A `None` group did not take part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcodeMatch {
    pub start: usize,
    pub end: usize,
    pub groups: [Option<Range<usize>>; 7],
}

impl ShortcodeMatch {
    /// `$m` as `preg_replace_callback()` builds it: group 6 always takes part, so an unmatched group is `""`.
    pub fn groups_in(&self, haystack: &[u8]) -> MatchGroups {
        MatchGroups(std::array::from_fn(|i| {
            self.groups[i].clone().map(|r| haystack[r].to_vec()).unwrap_or_default()
        }))
    }
}

/// `$m`: the seven groups of one match, copied out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchGroups(pub [Vec<u8>; 7]);

impl MatchGroups {
    /// `$m` as a PHP list of seven strings.
    pub fn to_value(&self) -> crate::PhpValue {
        crate::PhpValue::Array(
            self.0
                .iter()
                .enumerate()
                .map(|(i, g)| (crate::PhpKey::Int(i as i64), crate::php_bytes(g.clone())))
                .collect(),
        )
    }
}
