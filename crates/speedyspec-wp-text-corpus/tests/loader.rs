//! The corpus loader reads WordPress's recorded values exactly, and its assertions cannot pass by accident.

use serde_json::json;
use speedyspec_wp_text_corpus::{Php, PhpKey, assert_corpus, cases, coverage, decode};

fn text(s: &str) -> Php {
    Php::Str(s.as_bytes().to_vec())
}

#[test]
fn decodes_wordpress_arrays() {
    let all = cases("wp_html_split");
    let case = all
        .iter()
        .find(|c| c.scenario == "wp_html_split/data_basic_features/1")
        .expect("the scenario is recorded");
    assert_eq!(
        case.returned,
        Php::Array(vec![
            (PhpKey::Int(0), text("abcd ")),
            (PhpKey::Int(1), text("<html>")),
            (PhpKey::Int(2), text(" efgh")),
        ])
    );
    assert_eq!(case.args, vec![text("abcd <html> efgh")]);
}

#[test]
fn decodes_bytes() {
    assert_eq!(decode(&json!({"bytes": "/w=="})), Php::Str(vec![0xFF]));
    assert_eq!(decode(&json!({"float": 1.5})), Php::Float(1.5));
    assert_eq!(decode(&json!(7)), Php::Int(7));
    assert_eq!(decode(&json!(null)), Php::Null);
}

#[test]
#[should_panic(expected = "no_such_function")]
fn assert_corpus_panics_without_cases() {
    assert_corpus("no_such_function", |_| Php::Null);
}

#[test]
#[should_panic(expected = "wp_html_split/data_basic_features/0")]
fn assert_corpus_reports_a_wrong_result() {
    assert_corpus("wp_html_split", |_| Php::Null);
}

#[test]
fn coverage_counts_skips() {
    let none = coverage(&[]);
    assert_eq!(none.covered, 0);
    assert_eq!(none.skipped["wp_html_split"], cases("wp_html_split").len());
    let split = coverage(&["wp_html_split"]);
    assert_eq!(split.covered, cases("wp_html_split").len());
    assert!(!split.skipped.contains_key("wp_html_split"));
}

fn reported(kind: &str, function: &str) -> Php {
    Php::Array(vec![(
        PhpKey::Int(0),
        Php::Array(vec![
            (PhpKey::Int(0), text(kind)),
            (PhpKey::Int(1), text(function)),
        ]),
    )])
}

fn case_with(diagnostics: Php) -> speedyspec_wp_text_corpus::Case {
    speedyspec_wp_text_corpus::Case {
        scenario: "seems_utf8/x/0".into(),
        function: "seems_utf8".into(),
        args: vec![text("a")],
        returned: Php::Bool(true),
        diagnostics,
    }
}

#[test]
fn a_missing_diagnostic_is_a_mismatch() {
    let cases = vec![case_with(reported("deprecated_function", "seems_utf8"))];
    let wrong = speedyspec_wp_text_corpus::mismatches(&cases, |_| {
        (Php::Bool(true), Php::Array(vec![]))
    });
    assert_eq!(wrong.len(), 1);
    assert!(wrong[0].contains("seems_utf8/x/0"), "{}", wrong[0]);
}

#[test]
fn matching_diagnostics_pass() {
    let cases = vec![case_with(reported("deprecated_function", "seems_utf8"))];
    let wrong = speedyspec_wp_text_corpus::mismatches(&cases, |_| {
        (Php::Bool(true), reported("deprecated_function", "seems_utf8"))
    });
    assert!(wrong.is_empty(), "{wrong:?}");
}
