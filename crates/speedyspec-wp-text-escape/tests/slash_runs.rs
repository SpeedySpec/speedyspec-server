//! `addslashes` and `stripslashes` copy runs between special bytes; they must equal the byte-at-a-time definitions.

use proptest::prelude::*;
use speedyspec_wp_text_escape::{addslashes, stripslashes};

fn add_reference(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &b in text {
        if matches!(b, b'\'' | b'"' | b'\\' | 0) {
            out.push(b'\\');
            out.push(if b == 0 { b'0' } else { b });
        } else {
            out.push(b);
        }
    }
    out
}

fn strip_reference(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if text[i] == b'\\' {
            if let Some(&next) = text.get(i + 1) {
                out.push(if next == b'0' { 0 } else { next });
            }
            i += 2;
        } else {
            out.push(text[i]);
            i += 1;
        }
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn slashing_matches_reference(input in proptest::collection::vec(prop_oneof![Just(b'\\'), Just(b'\''), Just(b'"'), Just(0u8), Just(b'0'), Just(b'a')], 0..24)) {
        prop_assert_eq!(addslashes(&input).into_owned(), add_reference(&input));
        prop_assert_eq!(stripslashes(&input).into_owned(), strip_reference(&input));
    }
}
