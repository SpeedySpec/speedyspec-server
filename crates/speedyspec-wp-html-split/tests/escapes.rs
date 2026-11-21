//! The texturize split: comments are escaped, CDATA is not.

use speedyspec_wp_html_split::{Escapes, split_with, tag_end};

fn chunks(input: &[u8], escapes: Escapes) -> Vec<&[u8]> {
    split_with(input, escapes).map(|c| c.bytes()).collect()
}

#[test]
fn comments_only_treats_cdata_as_an_element() {
    assert_eq!(chunks(b"a<![CDATA[x>y]]>b", Escapes::CommentsOnly), vec![&b"a"[..], b"<![CDATA[x>", b"y]]>b"]);
    assert_eq!(chunks(b"a<![CDATA[x>y]]>b", Escapes::CommentsAndCdata), vec![&b"a"[..], b"<![CDATA[x>y]]>", b"b"]);
}

#[test]
fn comments_still_escape() {
    assert_eq!(chunks(b"a<!-- > -->b", Escapes::CommentsOnly), vec![&b"a"[..], b"<!-- > -->", b"b"]);
    assert_eq!(tag_end(b"<!-- x", 0, Escapes::CommentsOnly), 6);
    assert_eq!(tag_end(b"x<b>y", 1, Escapes::CommentsOnly), 4);
}
