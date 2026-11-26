//! Each matcher agrees with PCRE, as PHP runs kses's own patterns, on a fixed table of edge strings.

use serde_json::Value;
use speedyspec_wp_kses_lexical::scan::*;
use speedyspec_wp_kses_testkit::{PhpOracle, bytes, from_bytes};

const SUBJECTS: &[&[u8]] = &[
    b"",
    b"<",
    b">",
    b"<>",
    b"<a>",
    b"<a",
    b"a<b>c",
    b"<!-->",
    b"<!---->",
    b"<!--x-->",
    b"<!--x",
    b"<!--x\n",
    b"<!--x\ny-->",
    b"<!--x\n\n",
    b"</1>",
    b"</>>",
    b"</>",
    b"</ a>",
    b"</\n>",
    b"<!x>",
    b"<!X y>",
    b"<!>",
    b"<!",
    b"< / b >",
    b"<a href=x>",
    b"<a>\n",
    b"<a>\n\n",
    b"<a>x",
    b"<a-b c>",
    b"<9>",
    b"<-->",
    b"x>y",
    b"<<a>>",
    b"<a\n>",
    b"<a \x0B>",
    b"<a/>",
    b"<br />",
    b"<br / >",
    b"<br/ x>",
    b"a\xC3(<b>",
    b"<p\x00>",
    b"<!--a--b-->",
    b"<!-- - -->",
    b"<!--->",
    b"<a b='>'>",
    b"<\tp>",
];

fn preg(php: &mut PhpOracle, pattern: &str, subject: &[u8]) -> Option<(usize, usize)> {
    let r = php
        .call("preg", vec![Value::String(pattern.into()), bytes(subject)])
        .unwrap();
    let whole = r
        .returned
        .get("array")?
        .as_array()?
        .first()?
        .get(1)?
        .get("array")?
        .as_array()?
        .clone();
    let text = from_bytes(&whole[0][1]);
    let offset = whole[1][1].as_u64()? as usize;
    Some((offset, offset + text.len()))
}

const SPLIT: &str = "~(<!--.*?(-->|$))|</[^a-zA-Z][^>]*>|<![^>]*>|(<[^>]*(>|$)|>)~";

#[test]
fn next_token_matches_pcre() {
    let mut php = PhpOracle::start().unwrap();
    for s in SUBJECTS {
        let expected = preg(&mut php, SPLIT, s);
        let actual = next_token(s, 0).map(|(a, b, _)| (a, b));
        assert_eq!(actual, expected, "subject {:?}", String::from_utf8_lossy(s));
    }
}

#[test]
fn tag_head_matches_pcre() {
    let mut php = PhpOracle::start().unwrap();
    for s in SUBJECTS {
        let expected = preg(&mut php, "%^<\\s*(/\\s*)?([a-zA-Z0-9-]+)([^>]*)>?$%", s).is_some();
        assert_eq!(
            tag_head(s).is_some(),
            expected,
            "subject {:?}",
            String::from_utf8_lossy(s)
        );
    }
}

#[test]
fn invalid_wrapper_matches_pcre() {
    let mut php = PhpOracle::start().unwrap();
    for s in SUBJECTS {
        let expected = preg(&mut php, "~^(?:</[^a-zA-Z][^>]*>|<![a-z][^>]*>)$~", s).is_some();
        assert_eq!(
            invalid_wrapper(s),
            expected,
            "subject {:?}",
            String::from_utf8_lossy(s)
        );
    }
}

#[test]
fn xhtml_slash_matches_pcre() {
    let mut php = PhpOracle::start().unwrap();
    for s in SUBJECTS
        .iter()
        .chain([&b" / \n"[..], b"/\n\n", b"a/b"].iter())
    {
        let expected = preg(&mut php, "%\\s*/\\s*$%", s).is_some();
        assert_eq!(
            xhtml_slash(s),
            expected,
            "subject {:?}",
            String::from_utf8_lossy(s)
        );
    }
}
