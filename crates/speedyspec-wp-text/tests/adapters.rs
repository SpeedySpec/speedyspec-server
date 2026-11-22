//! The default adapters: they answer as WordPress's own functions would.

use speedyspec_wp_text::adapters::{FixedSmilies, HostLinkContext, PhpMtRand};
use speedyspec_wp_text_contracts::{LinkContext, RandomSource, Smilies};
use speedyspec_wp_text_corpus::{Php, cases};

#[path = "common/api.rs"]
mod api;

#[test]
fn host_link_context_matches_wp_is_internal_link() {
    let h = api::harness();
    let links = HostLinkContext::new(h.kses.clone(), b"http://example.org/blog");
    assert!(links.is_internal_link(b"http://EXAMPLE.org/a").unwrap());
    assert!(links.is_internal_link(b"https://example.org").unwrap());
    assert!(!links.is_internal_link(b"mailto:x@example.org").unwrap());
    assert!(!links.is_internal_link(b"http://evil.org").unwrap());
    assert!(!links.is_internal_link(b"//example.org/x").unwrap());
}

#[test]
fn php_mt_rand_matches_php() {
    // From PHP 8.5.12: mt_srand(1), then five rand(0, 2).
    let rand = PhpMtRand::seeded(1);
    let got: Vec<i64> = (0..5).map(|_| rand.rand(0, 2).unwrap()).collect();
    assert_eq!(got, vec![1, 2, 0, 2, 1]);
    // mt_srand(7), then rand(-5, 5), rand(10, 3) and rand(0, PHP_INT_MAX).
    let rand = PhpMtRand::seeded(7);
    assert_eq!(rand.rand(-5, 5).unwrap(), -2);
    assert_eq!(rand.rand(10, 3).unwrap(), 7);
    assert_eq!(rand.rand(0, i64::MAX).unwrap(), 5_883_999_053_047_972_377);
}

#[test]
fn fixed_smilies_search_matches_smilies_init() {
    let recorded = cases("speedyspec_smilies_search");
    let smilies = FixedSmilies::wordpress(api::INCLUDES, api::HOME);
    let search = smilies.search().unwrap().expect("a search");
    assert_eq!(Php::Str(search), recorded[0].returned);
}
