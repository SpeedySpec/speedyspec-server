//! Differential fuzz: the hand scanner and the reference regex agree on every generated haystack and tag set.

use proptest::prelude::*;
use speedyspec_wp_shortcode::{FancyRegexScanner, HandScanner};
use speedyspec_wp_shortcode_contracts::{ShortcodeScanner, TagName, TagSet};

const BYTES: &[u8] = b"[]/-.abAx\"= \n\xC3";
const SAFE: &[&[u8]] = &[b"a", b"b", b"a-b", b"a.b", b"ab"];

fn content() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(prop::sample::select(BYTES), 0..40)
}

fn tags(global: bool) -> impl Strategy<Value = Vec<&'static [u8]>> {
    let pool: Vec<&'static [u8]> = if global { [SAFE, &[b"a]".as_slice(), b"".as_slice()]].concat() } else { SAFE.to_vec() };
    prop::sample::subsequence(pool.clone(), 0..=3).prop_shuffle()
}

fn agree(content: &[u8], names: &[&[u8]]) -> Result<(), TestCaseError> {
    let set = TagSet::new(names.iter().map(|n| TagName::from_global(n.to_vec())).collect(), 1);
    let hand: Vec<_> = HandScanner::new().scan(content, &set).map(|m| m.unwrap()).collect();
    let fancy: Vec<_> = FancyRegexScanner::new().scan(content, &set).map(|m| m.unwrap()).collect();
    prop_assert_eq!(hand, fancy, "content {:?} tags {:?}", String::from_utf8_lossy(content), names);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn hand_agrees_with_reference(content in content(), names in tags(false)) {
        agree(&content, &names)?;
    }

    #[test]
    fn hand_agrees_with_reference_on_global_names(content in content(), names in tags(true)) {
        agree(&content, &names)?;
    }
}
