use proptest::prelude::*;
use regex::Regex;
use speedyspec_router::dispatch::{ChunkedAlternation, EngineChoice, RegexSetStrategy, engine_for};
use speedyspec_routing::DispatchStrategy;

fn strategies() -> Vec<Box<dyn DispatchStrategy>> {
    vec![
        Box::new(ChunkedAlternation { chunk_size: 10 }),
        Box::new(RegexSetStrategy { chunk_size: 10 }),
    ]
}

fn list(patterns: &[&str]) -> Vec<String> {
    patterns.iter().map(|p| p.to_string()).collect()
}

#[test]
fn first_match_wins_in_order() {
    for strategy in strategies() {
        let compiled = strategy.build(&list(&["^a.*", "^ab"])).unwrap();
        assert_eq!(compiled.find("ab", 0).map(|(i, _)| i), Some(0));
    }
}

#[test]
fn captures_belong_to_the_winner() {
    for strategy in strategies() {
        let compiled = strategy
            .build(&list(&["^x/([0-9]+)$", "^([a-z]+)/([a-z]+)$", "^(.*)$"]))
            .unwrap();
        let (index, caps) = compiled.find("ab/cd", 0).unwrap();
        assert_eq!(index, 1);
        assert_eq!(
            caps,
            vec![Some("ab/cd".into()), Some("ab".into()), Some("cd".into())]
        );
    }
}

#[test]
fn unmatched_optional_group_is_none() {
    for strategy in strategies() {
        let compiled = strategy.build(&list(&["^a(?:/([0-9]+))?$"])).unwrap();
        assert_eq!(
            compiled.find("a", 0).unwrap().1,
            vec![Some("a".into()), None]
        );
    }
}

#[test]
fn find_from_skips_earlier() {
    for strategy in strategies() {
        let compiled = strategy.build(&list(&["^a", "^a", "^b", "^a"])).unwrap();
        assert_eq!(compiled.find("a", 1).map(|(i, _)| i), Some(1));
        assert_eq!(compiled.find("a", 2).map(|(i, _)| i), Some(3));
        assert_eq!(compiled.find("a", 4), None);
    }
}

#[test]
fn fancy_rule_keeps_order() {
    assert!(matches!(engine_for("^(?=foo)foo/bar"), EngineChoice::Fancy));
    for strategy in strategies() {
        let compiled = strategy
            .build(&list(&["^nope", "^(?=foo)foo/bar", "^foo/.*"]))
            .unwrap();
        assert_eq!(compiled.find("foo/bar", 0).map(|(i, _)| i), Some(1));
        assert_eq!(compiled.find("foo/baz", 0).map(|(i, _)| i), Some(2));
    }
}

#[test]
fn uncompilable_rule_marks_needs_php() {
    assert!(matches!(
        engine_for("^(?|(a)|(b))/?$"),
        EngineChoice::None(_)
    ));
    for strategy in strategies() {
        let compiled = strategy
            .build(&list(&["^x", "^(?|(a)|(b))/?$", "^b"]))
            .unwrap();
        assert_eq!(compiled.first_needs_php(0), Some(1));
        assert_eq!(compiled.first_needs_php(2), None);
        assert_eq!(compiled.find("b", 0).map(|(i, _)| i), Some(2));
    }
}

#[test]
fn oversized_chunk_is_split() {
    let patterns: Vec<String> = (0..2000)
        .map(|i| format!("^prefix-{i}/([a-z0-9-]+)/?$"))
        .collect();
    for strategy in [
        Box::new(ChunkedAlternation { chunk_size: 2000 }) as Box<dyn DispatchStrategy>,
        Box::new(RegexSetStrategy { chunk_size: 2000 }),
    ] {
        let compiled = strategy.build(&patterns).unwrap();
        let (index, caps) = compiled.find("prefix-1999/hello", 0).unwrap();
        assert_eq!(index, 1999);
        assert_eq!(caps[1].as_deref(), Some("hello"));
    }
}

fn one_per_pattern(patterns: &[String], subject: &str) -> Option<(usize, Vec<Option<String>>)> {
    patterns.iter().enumerate().find_map(|(i, p)| {
        let re = Regex::new(p).unwrap();
        re.captures(subject).map(|c| {
            (
                i,
                c.iter()
                    .map(|m| m.map(|m| m.as_str().to_string()))
                    .collect(),
            )
        })
    })
}

fn piece() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("a".to_string()),
        Just("b/".to_string()),
        Just("[0-9]+".to_string()),
        Just("([^/]+)".to_string()),
        Just("(.+?)".to_string()),
        Just("/?$".to_string()),
        Just("(?:/([0-9]+))?".to_string()),
    ]
}

proptest! {
    #[test]
    fn strategies_agree(
        patterns in prop::collection::vec(prop::collection::vec(piece(), 1..4).prop_map(|p| format!("^{}", p.concat())), 1..60),
        subject in "[ab0-9/]{0,8}",
    ) {
        let expected = one_per_pattern(&patterns, &subject);
        for strategy in strategies() {
            let compiled = strategy.build(&patterns).unwrap();
            prop_assert_eq!(compiled.find(&subject, 0), expected.clone());
        }
    }
}
