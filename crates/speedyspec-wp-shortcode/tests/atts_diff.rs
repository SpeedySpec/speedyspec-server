//! Differential fuzz: the hand attribute parser and the regex one agree on every generated text.

use proptest::prelude::*;
use speedyspec_wp_shortcode::{HandAttsParser, RegexAttsParser};
use speedyspec_wp_shortcode_contracts::AttsParser;

const PIECES: &[&[u8]] = &[
    b"a", b"0", b"1", b"=", b"\"", b"'", b"\\", b"<", b">", b"-", b" ", b"\n", b"\t", b"\xC2\xA0", b"x", b"\x0B",
];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn hand_agrees_with_regex(pieces in prop::collection::vec(prop::sample::select(PIECES), 0..30)) {
        let text = pieces.concat();
        prop_assert_eq!(HandAttsParser::new().parse(&text), RegexAttsParser::new().parse(&text), "text {:?}", String::from_utf8_lossy(&text));
    }
}
