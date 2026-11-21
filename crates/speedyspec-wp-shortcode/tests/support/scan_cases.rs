//! Scanner cases, each expected value taken from PHP 8.5's `preg_match_all` over WordPress's `get_shortcode_regex`.

use std::ops::Range;

/// One haystack, the tag set in alternation order, and every match's seven group spans.
pub struct ScanCase {
    pub content: &'static [u8],
    pub tags: &'static [&'static [u8]],
    pub expected: Vec<[Option<Range<usize>>; 7]>,
}

pub fn scan_cases() -> Vec<ScanCase> {
    vec![
        ScanCase { content: b"[a]", tags: &[b"a"], expected: vec![[Some(0..3), Some(1..1), Some(1..2), Some(2..2), None, None, Some(3..3)]] },
        ScanCase { content: b"[a/]", tags: &[b"a"], expected: vec![[Some(0..4), Some(1..1), Some(1..2), Some(2..2), Some(2..3), None, Some(4..4)]] },
        ScanCase { content: b"[a /]", tags: &[b"a"], expected: vec![[Some(0..5), Some(1..1), Some(1..2), Some(2..3), Some(3..4), None, Some(5..5)]] },
        ScanCase { content: b"[a x=\x22/\x22]", tags: &[b"a"], expected: vec![[Some(0..9), Some(1..1), Some(1..2), Some(2..8), None, None, Some(9..9)]] },
        ScanCase { content: b"[a x=\x22/\x22/]", tags: &[b"a"], expected: vec![[Some(0..10), Some(1..1), Some(1..2), Some(2..8), Some(8..9), None, Some(10..10)]] },
        ScanCase { content: b"[a]x[/a]", tags: &[b"a"], expected: vec![[Some(0..8), Some(1..1), Some(1..2), Some(2..2), None, Some(3..4), Some(8..8)]] },
        ScanCase { content: b"[a]x[/A]", tags: &[b"a"], expected: vec![[Some(0..3), Some(1..1), Some(1..2), Some(2..2), None, None, Some(3..3)]] },
        ScanCase { content: b"[[a]]", tags: &[b"a"], expected: vec![[Some(0..5), Some(1..2), Some(2..3), Some(3..3), None, None, Some(4..5)]] },
        ScanCase { content: b"[[a]", tags: &[b"a"], expected: vec![[Some(0..4), Some(1..2), Some(2..3), Some(3..3), None, None, Some(4..4)]] },
        ScanCase { content: b"[a]]", tags: &[b"a"], expected: vec![[Some(0..4), Some(1..1), Some(1..2), Some(2..2), None, None, Some(3..4)]] },
        ScanCase { content: b"[a]x[a]y[/a]", tags: &[b"a"], expected: vec![[Some(0..12), Some(1..1), Some(1..2), Some(2..2), None, Some(3..8), Some(12..12)]] },
        ScanCase { content: b"[a-b]", tags: &[b"a", b"a-b"], expected: vec![[Some(0..5), Some(1..1), Some(1..4), Some(4..4), None, None, Some(5..5)]] },
        ScanCase { content: b"[a.b]", tags: &[b"a", b"a.b"], expected: vec![[Some(0..5), Some(1..1), Some(1..2), Some(2..4), None, None, Some(5..5)]] },
        ScanCase { content: b"[a.b]", tags: &[b"a.b", b"a"], expected: vec![[Some(0..5), Some(1..1), Some(1..4), Some(4..4), None, None, Some(5..5)]] },
        ScanCase { content: b"[a]", tags: &[b"a]", b"a"], expected: vec![[Some(0..3), Some(1..1), Some(1..2), Some(2..2), None, None, Some(3..3)]] },
        ScanCase { content: b"[/x]", tags: &[], expected: vec![[Some(0..4), Some(1..1), Some(1..1), Some(1..3), None, None, Some(4..4)]] },
        ScanCase { content: b"[a", tags: &[b"a"], expected: vec![] },
        ScanCase { content: b"[a][a]", tags: &[b"a"], expected: vec![[Some(0..3), Some(1..1), Some(1..2), Some(2..2), None, None, Some(3..3)], [Some(3..6), Some(4..4), Some(4..5), Some(5..5), None, None, Some(6..6)]] },
        ScanCase { content: b"[ab]", tags: &[b"a"], expected: vec![] },
        ScanCase { content: b"[a_b]", tags: &[b"a"], expected: vec![] },
        ScanCase { content: b"[a]x[/a] [a]y[/a]", tags: &[b"a"], expected: vec![[Some(0..8), Some(1..1), Some(1..2), Some(2..2), None, Some(3..4), Some(8..8)], [Some(9..17), Some(10..10), Some(10..11), Some(11..11), None, Some(12..13), Some(17..17)]] },
        ScanCase { content: b"[a b=1]c[/a]", tags: &[b"a", b"b"], expected: vec![[Some(0..12), Some(1..1), Some(1..2), Some(2..6), None, Some(7..8), Some(12..12)]] },
        ScanCase { content: b"[b][a][/b]", tags: &[b"a", b"b"], expected: vec![[Some(0..10), Some(1..1), Some(1..2), Some(2..2), None, Some(3..6), Some(10..10)]] },
        ScanCase { content: b"[a]\xff[/a]", tags: &[b"a"], expected: vec![[Some(0..8), Some(1..1), Some(1..2), Some(2..2), None, Some(3..4), Some(8..8)]] },
        ScanCase { content: b"[[a]x[/a]]", tags: &[b"a"], expected: vec![[Some(0..10), Some(1..2), Some(2..3), Some(3..3), None, Some(4..5), Some(9..10)]] },
        ScanCase { content: b"[a]x[/a]]", tags: &[b"a"], expected: vec![[Some(0..9), Some(1..1), Some(1..2), Some(2..2), None, Some(3..4), Some(8..9)]] },
        ScanCase { content: b"[a/ ]", tags: &[b"a"], expected: vec![[Some(0..5), Some(1..1), Some(1..2), Some(2..4), None, None, Some(5..5)]] },
        ScanCase { content: b"[a x=/]", tags: &[b"a"], expected: vec![[Some(0..7), Some(1..1), Some(1..2), Some(2..5), Some(5..6), None, Some(7..7)]] },
        ScanCase { content: b"[a/x]", tags: &[b"a"], expected: vec![[Some(0..5), Some(1..1), Some(1..2), Some(2..4), None, None, Some(5..5)]] },
        ScanCase { content: b"[[[a]]]", tags: &[b"a"], expected: vec![[Some(1..6), Some(2..3), Some(3..4), Some(4..4), None, None, Some(5..6)]] },
        ScanCase { content: b"[a]][/a]", tags: &[b"a"], expected: vec![[Some(0..8), Some(1..1), Some(1..2), Some(2..2), None, Some(3..4), Some(8..8)]] },
        ScanCase { content: b"x[/a]", tags: &[b"a"], expected: vec![] },
        ScanCase { content: b"[a]x[ /a]", tags: &[b"a"], expected: vec![[Some(0..3), Some(1..1), Some(1..2), Some(2..2), None, None, Some(3..3)]] },
        ScanCase { content: b"[e\x5cu{301}]", tags: &[b"e\xcc\x81"], expected: vec![] },
        ScanCase { content: b"[a x=\x22]\x22]y[/a]", tags: &[b"a"], expected: vec![[Some(0..14), Some(1..1), Some(1..2), Some(2..6), None, Some(7..10), Some(14..14)]] },
        ScanCase { content: b"[a][/a][/a]", tags: &[b"a"], expected: vec![[Some(0..7), Some(1..1), Some(1..2), Some(2..2), None, Some(3..3), Some(7..7)]] },
    ]
}
