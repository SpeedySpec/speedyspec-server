//! The contracts' plain types and WordPress's pattern strings, pinned to PHP 8.5 and WordPress.

use speedyspec_wp_shortcode_contracts::{
    AttKey, InvalidTagName, ParsedAtts, ShortcodeMatch, TagName, TagSet, get_shortcode_atts_regex,
    get_shortcode_regex, preg_quote, shortcode_tag_alternation,
};

fn names(list: &[&str]) -> Vec<TagName> {
    list.iter().map(|n| TagName::new(n.as_bytes()).unwrap()).collect()
}

#[test]
fn tag_name_rejects_empty_and_reserved() {
    assert_eq!(TagName::new(b"").unwrap_err(), InvalidTagName::Empty);
    assert_eq!(TagName::new(b" \t\n\r\0\x0B").unwrap_err(), InvalidTagName::Empty);
    for bad in [b'<', b'>', b'&', b'/', b'[', b']', b'=', 0x00, 0x20, 0x1f] {
        let name = [b'a', bad, b'b'];
        assert_eq!(TagName::new(&name).unwrap_err(), InvalidTagName::Reserved, "byte {bad:#x}");
    }
    for good in ["a.b", "a-b", "a+", "caption", "\u{e9}t\u{e9}"] {
        assert_eq!(TagName::new(good.as_bytes()).unwrap().as_bytes(), good.as_bytes());
    }
}

#[test]
fn tag_name_from_global_is_unvalidated() {
    assert_eq!(TagName::from_global(b"a]".to_vec()).as_bytes(), b"a]");
}

#[test]
fn tag_set_flags_unsafe_names() {
    assert!(!TagSet::new(names(&["a", "a.b"]), 1).has_unsafe_names());
    assert!(TagSet::new(vec![], 1).has_unsafe_names());
    assert!(TagSet::new(vec![TagName::from_global(b"a]".to_vec())], 1).has_unsafe_names());
    assert!(TagSet::new(vec![TagName::from_global(b"a/b".to_vec())], 1).has_unsafe_names());
}

#[test]
fn att_key_numeric_strings_follow_php() {
    assert_eq!(AttKey::from_php(b"1".to_vec()), AttKey::Int(1));
    assert_eq!(AttKey::from_php(b"-1".to_vec()), AttKey::Int(-1));
    assert_eq!(AttKey::from_php(b"9223372036854775807".to_vec()), AttKey::Int(i64::MAX));
    for s in ["01", "1.0", "-0", "9223372036854775808", "", " 1", "a"] {
        assert_eq!(AttKey::from_php(s.as_bytes().to_vec()), AttKey::Str(s.as_bytes().to_vec()), "{s:?}");
    }
}

#[test]
fn parsed_atts_follow_php_array_order() {
    let mut atts = ParsedAtts::new();
    atts.insert(AttKey::Str(b"a".to_vec()), b"1".to_vec());
    atts.push(b"x".to_vec());
    atts.insert(AttKey::Int(5), b"five".to_vec());
    atts.push(b"y".to_vec());
    atts.insert(AttKey::Str(b"a".to_vec()), b"2".to_vec());
    let got: Vec<(AttKey, Vec<u8>)> = atts.iter().map(|(k, v)| (k.clone(), v.to_vec())).collect();
    assert_eq!(
        got,
        vec![
            (AttKey::Str(b"a".to_vec()), b"2".to_vec()),
            (AttKey::Int(0), b"x".to_vec()),
            (AttKey::Int(5), b"five".to_vec()),
            (AttKey::Int(6), b"y".to_vec()),
        ]
    );
}

#[test]
fn parsed_atts_negative_first_key_continues_from_it() {
    let mut atts = ParsedAtts::new();
    atts.insert(AttKey::Int(-5), b"n".to_vec());
    atts.push(b"x".to_vec());
    assert_eq!(atts.iter().nth(1).unwrap().0, &AttKey::Int(-4));
}

#[test]
fn preg_quote_matches_php_85() {
    assert_eq!(
        preg_quote(b".\\+*?[^]$(){}=!<>|:-#/\0z"),
        b"\\.\\\\\\+\\*\\?\\[\\^\\]\\$\\(\\)\\{\\}\\=\\!\\<\\>\\|\\:\\-\\#/\\000z".to_vec()
    );
}

#[test]
fn alternation_joins_quoted_names_in_order() {
    assert_eq!(shortcode_tag_alternation(&names(&["gallery", "a.b"])), b"gallery|a\\.b".to_vec());
}

#[test]
fn shortcode_regex_matches_wordpress() {
    assert_eq!(
        String::from_utf8(get_shortcode_regex(&names(&["gallery", "a.b"]))).unwrap(),
        r"\[(\[?)(gallery|a\.b)(?![\w-])([^\]\/]*(?:\/(?!\])[^\]\/]*)*?)(?:(\/)\]|\](?:([^\[]*+(?:\[(?!\/\2\])[^\[]*+)*+)\[\/\2\])?)(\]?)"
    );
    assert_eq!(
        String::from_utf8(get_shortcode_regex(&[])).unwrap(),
        r"\[(\[?)()(?![\w-])([^\]\/]*(?:\/(?!\])[^\]\/]*)*?)(?:(\/)\]|\](?:([^\[]*+(?:\[(?!\/\2\])[^\[]*+)*+)\[\/\2\])?)(\]?)"
    );
}

#[test]
fn atts_regex_matches_wordpress() {
    assert_eq!(
        std::str::from_utf8(get_shortcode_atts_regex()).unwrap(),
        r#"/([\w-]+)\s*=\s*"([^"]*)"(?:\s|$)|([\w-]+)\s*=\s*'([^']*)'(?:\s|$)|([\w-]+)\s*=\s*([^\s'"]+)(?:\s|$)|"([^"]*)"(?:\s|$)|'([^']*)'(?:\s|$)|(\S+)(?:\s|$)/"#
    );
}

#[test]
fn match_groups_give_empty_for_unmatched() {
    let hay = b"x[a]y";
    let m = ShortcodeMatch {
        start: 1,
        end: 4,
        groups: [Some(1..4), Some(2..2), Some(2..3), Some(3..3), None, None, Some(4..4)],
    };
    let g = m.groups_in(hay);
    assert_eq!(g.0[0], b"[a]".to_vec());
    assert_eq!(g.0[2], b"a".to_vec());
    assert_eq!(g.0[4], Vec::<u8>::new());
    assert_eq!(g.0[5], Vec::<u8>::new());
}
