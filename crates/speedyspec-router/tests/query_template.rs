use speedyspec_router::wordpress::{QueryTemplate, php_parse_str};
use speedyspec_routing::QueryValue;

fn text(s: &str) -> QueryValue {
    QueryValue::Text(s.to_string())
}

#[test]
fn strips_up_to_question_mark() {
    let template = QueryTemplate::parse("index.php?name=$matches[1]");
    assert_eq!(
        template.apply(&[Some("x".into()), Some("hello".into())]),
        "name=hello"
    );
}

#[test]
fn substitutes_and_urlencodes() {
    let template = QueryTemplate::parse("index.php?name=$matches[1]&page=$matches[2]");
    let caps = [Some("x".into()), Some("héllo wörld".into()), None];
    assert_eq!(template.apply(&caps), "name=h%C3%A9llo+w%C3%B6rld&page=");
}

#[test]
fn out_of_range_match_is_empty() {
    let template = QueryTemplate::parse("index.php?a=$matches[5]");
    assert_eq!(template.apply(&[Some("x".into())]), "a=");
}

#[test]
fn matches_zero_is_literal() {
    let template = QueryTemplate::parse("index.php?a=$matches[0]&b=$matches[10]");
    let mut caps: Vec<Option<String>> = vec![Some("whole".into())];
    caps.extend((1..=10).map(|i| Some(format!("v{i}"))));
    assert_eq!(template.apply(&caps), "a=$matches[0]&b=v10");
}

#[test]
fn addslashes_after_substitution() {
    let template = QueryTemplate::parse("index.php?a=$matches[1]&b='q'");
    assert_eq!(
        template.apply(&[None, Some("a'b".into())]),
        "a=a%27b&b=\\'q\\'"
    );
}

#[test]
fn parse_str_nests() {
    assert_eq!(
        php_parse_str("a[]=1&a[]=2&b[c]=3"),
        vec![
            (
                "a".to_string(),
                QueryValue::Map(vec![("0".into(), text("1")), ("1".into(), text("2"))])
            ),
            (
                "b".to_string(),
                QueryValue::Map(vec![("c".into(), text("3"))])
            ),
        ]
    );
}

#[test]
fn parse_str_mangles_dots_and_spaces() {
    assert_eq!(
        php_parse_str("a.b=1&c d=2"),
        vec![("a_b".into(), text("1")), ("c_d".into(), text("2"))]
    );
}

#[test]
fn parse_str_plus_is_space_and_last_wins() {
    assert_eq!(
        php_parse_str("a=x+y&a=%7Ez&b"),
        vec![("a".into(), text("~z")), ("b".into(), text(""))]
    );
}

#[test]
fn matches_map_substitutes_without_stripping_or_slashes() {
    use speedyspec_router::wordpress::matches_map;
    assert_eq!(
        matches_map("a=$matches[1]", &[Some("".into()), Some("b c".into())]),
        "a=b+c"
    );
    assert_eq!(
        matches_map("index.php?x='$matches[1]'", &[None, Some("y".into())]),
        "index.php?x='y'"
    );
}

/// `preg_replace('!^.+\?!', '', $query)` is greedy on the first line: it trims through the last `?` there.
/// Expected values are PHP's own.
#[test]
fn strips_through_the_last_question_mark_of_the_first_line() {
    let cases = [
        ("index.php?a=1?b=2", "b=2"),
        ("?a?b", "b"),
        ("?a", "?a"),
        ("x?a\ny?b", "a\\ny?b"),
    ];
    for (query, expected) in cases {
        let applied = QueryTemplate::parse(query).apply(&[Some(String::new())]);
        assert_eq!(applied.replace('\n', "\\n"), expected, "{query:?}");
    }
}
