//! `wp_html_split()` splits exactly as `preg_split( get_html_split_regex(), $input, -1, PREG_SPLIT_DELIM_CAPTURE )`.

use speedyspec_wp_html_split::{HTML_SPLIT_REGEX, HtmlChunk, split, wp_html_split};

fn check(input: &str, expected: &[&str]) {
    let got: Vec<&[u8]> = wp_html_split(input.as_bytes());
    let want: Vec<&[u8]> = expected.iter().map(|s| s.as_bytes()).collect();
    assert_eq!(got, want, "splitting {input:?}");
}

// From WordPress's data_basic_features.

#[test]
fn basic_text() {
    check("abcd efgh", &["abcd efgh"]);
}

#[test]
fn basic_tag() {
    check("abcd <html> efgh", &["abcd ", "<html>", " efgh"]);
}

#[test]
fn basic_comment() {
    check("abcd <!-- <html> --> efgh", &["abcd ", "<!-- <html> -->", " efgh"]);
}

#[test]
fn basic_cdata() {
    check(
        "abcd <![CDATA[ <html> ]]> efgh",
        &["abcd ", "<![CDATA[ <html> ]]>", " efgh"],
    );
}

// Malformed input at the end runs to the end of the input.

#[test]
fn unterminated_lt() {
    check("a<", &["a", "<", ""]);
}

#[test]
fn unterminated_tag() {
    check("x <a href=\"y\"", &["x ", "<a href=\"y\"", ""]);
}

#[test]
fn unterminated_comment() {
    check("x <!-- a <b> c", &["x ", "<!-- a <b> c", ""]);
}

#[test]
fn unterminated_cdata() {
    check("x <![CDATA[ a ]]", &["x ", "<![CDATA[ a ]]", ""]);
}

#[test]
fn comment_bang_close() {
    check("<!-- a --!> b", &["", "<!-- a --!> b", ""]);
}

// Empty text chunks before, between and after tags.

#[test]
fn empty() {
    check("", &[""]);
}

#[test]
fn adjacent() {
    check("<a><b>", &["", "<a>", "", "<b>", ""]);
}

// Only `<!--` and `<![CDATA[` start an escaped element.

#[test]
fn near_miss_bang() {
    check("<!-x>y", &["", "<!-x>", "y"]);
}

#[test]
fn short_comment() {
    check("<!-->z", &["", "<!-->", "z"]);
}

#[test]
fn dash_comment() {
    check("<!--->z", &["", "<!--->", "z"]);
}

#[test]
fn triple_dash_end() {
    check("<!-- a --->z", &["", "<!-- a --->", "z"]);
}

#[test]
fn cdata_near_miss() {
    check("<![CDATA x>y", &["", "<![CDATA x>", "y"]);
}

#[test]
fn gt_inside_comment() {
    check("<!-- a > b -->c", &["", "<!-- a > b -->", "c"]);
}

#[test]
fn split_alternates() {
    let chunks: Vec<HtmlChunk<'_>> = split(b"<a>b<c>").collect();
    assert_eq!(
        chunks,
        vec![
            HtmlChunk::Text(b""),
            HtmlChunk::Tag(b"<a>"),
            HtmlChunk::Text(b"b"),
            HtmlChunk::Tag(b"<c>"),
            HtmlChunk::Text(b""),
        ]
    );
}

#[test]
fn regex_string_is_wordpress() {
    assert_eq!(
        HTML_SPLIT_REGEX,
        r"/(<(?(?=!--|!\[CDATA\[)(?(?=!-)!(?:-(?!->)[^\-]*+)*+(?:-->)?|!\[CDATA\[[^\]]*+(?:](?!]>)[^\]]*+)*+(?:]]>)?)|[^>]*>?))/"
    );
}
