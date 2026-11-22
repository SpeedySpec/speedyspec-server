//! `convert_smilies` and `translate_smiley`: the recorded corpus through `TextApi`, the `smilies_src` filter, and
//! the hand matcher against WordPress's pattern.

mod common;

use std::rc::Rc;

use common::text;
use proptest::prelude::*;
use speedyspec_wp_hooks::{Callback, HookName, HookValue};
use speedyspec_wp_text::adapters::{FixedSmilies, NoSmilies};
use speedyspec_wp_text::smilies::{SmileyEngine, convert_smilies_core};
use speedyspec_wp_text_contracts::Smilies;
use speedyspec_wp_text_corpus::{Php, assert_corpus};
use speedyspec_wp_text_memory::FixedSiteSettings;

#[path = "common/api.rs"]
mod api;

/// A recorded smiley table.
fn table(arg: &Php) -> Vec<(Vec<u8>, Vec<u8>)> {
    let Php::Array(items) = arg else { panic!("a table") };
    items
        .iter()
        .map(|(k, v)| {
            let key = match k {
                speedyspec_wp_text_corpus::PhpKey::Str(s) => s.clone(),
                speedyspec_wp_text_corpus::PhpKey::Int(i) => i.to_string().into_bytes(),
            };
            (key, text(v).to_vec())
        })
        .collect()
}

#[test]
fn convert_smilies_corpus() {
    let h = api::harness();
    assert_corpus("convert_smilies", |args| Php::Str(h.api.convert_smilies(text(&args[0])).unwrap().into_owned()));
}

#[test]
fn custom_table_corpus() {
    assert_corpus("speedyspec_convert_smilies_with", |args| {
        let smilies = Rc::new(FixedSmilies::new(table(&args[1]), api::INCLUDES.to_vec(), api::HOME.to_vec()));
        let h = api::harness_options(api::Options { smilies: Some(smilies), ..Default::default() });
        Php::Str(h.api.convert_smilies(text(&args[0])).unwrap().into_owned())
    });
}

#[test]
fn smilies_off_returns_input() {
    let settings = FixedSiteSettings { use_smilies: false, ..FixedSiteSettings::default() };
    let h = api::harness_options(api::Options { settings, ..Default::default() });
    assert_eq!(&*h.api.convert_smilies(b"a :) b").unwrap(), b"a :) b");
    let h = api::harness_options(api::Options { smilies: Some(Rc::new(NoSmilies)), ..Default::default() });
    assert_eq!(&*h.api.convert_smilies(b"a :) b").unwrap(), b"a :) b");
}

#[test]
fn smilies_src_filter_args() {
    let h = api::harness();
    let smilies: Rc<dyn Smilies> = Rc::new(FixedSmilies::new(vec![(b":)".to_vec(), b"s.png".to_vec())], api::INCLUDES.to_vec(), api::HOME.to_vec()));
    let h2 = api::harness_options(api::Options { smilies: Some(smilies), ..Default::default() });
    let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
    let log = seen.clone();
    h2.hooks
        .add_filter(
            &HookName::from("smilies_src"),
            Callback::rust("t", move |args: &[HookValue]| {
                log.borrow_mut().push(args.to_vec());
                Ok(args[0].clone())
            }),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    let out = h2.api.convert_smilies(b"hi :)").unwrap();
    assert_eq!(
        seen.borrow()[0],
        vec![
            HookValue::Str("https://example.org/wp-includes/images/smilies/s.png".into()),
            HookValue::Str("s.png".into()),
            HookValue::Str("http://example.org".into()),
        ]
    );
    assert_eq!(
        &*out,
        &b"hi <img src=\"https://example.org/wp-includes/images/smilies/s.png\" alt=\":)\" class=\"wp-smiley\" style=\"height: 1em; max-height: 1em;\" />"[..]
    );
    drop(h);
}

fn smiley_text() -> impl Strategy<Value = Vec<u8>> {
    let atoms: Vec<&'static [u8]> = vec![
        b":", b";", b"8", b"-", b")", b"(", b"D", b"P", b"o", b"x", b"|", b"?", b"!", b"O", b"mrgreen", b"lol", b" ", b"\n",
        b"\t", b"\xC2\xA0", b"&nbsp;", b"<", b">", b"code", b"pre", b"/", b"a",
    ];
    proptest::collection::vec(proptest::sample::select(atoms), 0..24).prop_map(|parts| parts.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn fast_smilies_match_pattern(input in smiley_text()) {
        let smilies = FixedSmilies::wordpress(api::INCLUDES, api::HOME);
        let search = smilies.search().unwrap().unwrap();
        let keys = smilies.smileys().unwrap();
        let hand = SmileyEngine::new(&search, &keys).expect("a valid search");
        prop_assert!(hand.is_hand());
        let reference = SmileyEngine::reference(&search).expect("a valid search");
        let mark = |m: &[u8]| [&b"["[..], m, b"]"].concat();
        let expected = convert_smilies_core(&input, &reference, &mut |m| mark(m));
        prop_assert_eq!(&*convert_smilies_core(&input, &hand, &mut |m| mark(m)), &*expected);
    }
}
