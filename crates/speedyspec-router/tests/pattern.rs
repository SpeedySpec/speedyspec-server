use regex::Regex;
use speedyspec_router::pattern::{PatternError, parse};

fn wheres(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn literal_is_static() {
    let pattern = parse("/robots.txt", &[]).unwrap();
    assert!(pattern.is_static);
    assert!(pattern.params.is_empty());
}

#[test]
fn param_defaults_to_segment() {
    let pattern = parse("/users/{id}", &[]).unwrap();
    assert!(!pattern.is_static);
    assert_eq!(pattern.regex, "^/users/([^/]+)$");
    assert_eq!(pattern.params, vec!["id".to_string()]);
}

#[test]
fn where_and_inline_are_one_constraint() {
    let with_where = parse("/u/{id}", &wheres(&[("id", "[0-9]+")])).unwrap();
    let inline = parse("/u/{id:[0-9]+}", &[]).unwrap();
    assert_eq!(with_where.regex, inline.regex);
    assert_eq!(inline.regex, "^/u/([0-9]+)$");
}

#[test]
fn inline_constraint_may_hold_braces() {
    let pattern = parse("/archive/{year:[0-9]{4}}", &[]).unwrap();
    assert_eq!(pattern.regex, "^/archive/([0-9]{4})$");
}

#[test]
fn optional_trailing() {
    let pattern = parse("/archive/{year}/{month?}", &[]).unwrap();
    let re = Regex::new(&pattern.regex).unwrap();
    assert!(re.is_match("/archive/2024"));
    assert!(re.is_match("/archive/2024/05"));
    assert!(!re.is_match("/archive"));
    assert_eq!(&re.captures("/archive/2024/05").unwrap()[2], "05");
}

#[test]
fn optional_not_trailing_is_rejected() {
    assert!(matches!(
        parse("/a/{x?}/b", &[]),
        Err(PatternError::OptionalNotTrailing(_))
    ));
    assert!(matches!(
        parse("/a/{x?}/{y}", &[]),
        Err(PatternError::OptionalNotTrailing(_))
    ));
}

#[test]
fn capturing_group_in_constraint_is_rejected() {
    assert!(matches!(
        parse("/a/{x:(a|b)}", &[]),
        Err(PatternError::CapturingGroupInConstraint(_))
    ));
    assert!(parse("/a/{x:(?:a|b)}", &[]).is_ok());
    assert!(parse(r"/a/{x:\(a\)}", &[]).is_ok());
}

#[test]
fn duplicate_and_unclosed_are_rejected() {
    assert!(matches!(
        parse("/{a}/{a}", &[]),
        Err(PatternError::DuplicateParam(_))
    ));
    assert!(matches!(
        parse("/{a", &[]),
        Err(PatternError::UnclosedBrace(_))
    ));
}

#[test]
fn literal_regex_chars_are_escaped() {
    let pattern = parse("/a.b/{x}", &[]).unwrap();
    let re = Regex::new(&pattern.regex).unwrap();
    assert!(re.is_match("/a.b/1"));
    assert!(!re.is_match("/axb/1"));
}
