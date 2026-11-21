//! A grammar-aware generator of kses input: markup pieces kses treats specially, mixed at random.

use proptest::prelude::*;

const TAGS: &[&str] = &[
    "p", "a", "img", "div", "span", "b", "em", "script", "style", "iframe", "svg", "math",
    "object", "form", "input", "table", "td", "ol", "li", "br", "x-y", "P", "A", "IMG", "1x", "",
];
const ATTRS: &[&str] = &[
    "href",
    "src",
    "title",
    "class",
    "style",
    "onclick",
    "onerror",
    "data-x",
    "data-wp-bind--href",
    "id",
    "alt",
    "srcdoc",
    "xlink:href",
    "colspan",
    "start",
    "HREF",
    "OnLoad",
    "data-",
    "_x",
    "a:b",
];
const VALUES: &[&str] = &[
    "x",
    "javascript:alert(1)",
    "JaVa&#x09;ScRiPt:x",
    "&#106;avascript:x",
    "java\0script:x",
    "feed:javascript:x",
    "http://a/?b=c&d=e",
    "data:text/html,<script>x</script>",
    "color:red",
    "background:url(javascript:x)",
    "width:calc(1px + (2px))",
    "expression(x)",
    "1",
    "100",
    "\"",
    "'",
    "<",
    ">",
    "&",
    "&amp;",
    "&bogus;",
    "&#xD800;",
    "/?x:y",
    "  x  ",
    "\u{a0}",
    "%6a",
];

const SPECIAL: &[&[u8]] = &[
    b"<!--",
    b"-->",
    b"<!-- c -->",
    b"</1",
    b"<!x",
    b"<!DOCTYPE x>",
    b"<!ENTITY a \"b\">",
    b"<?xml x?>",
    b"<![CDATA[x]]>",
    b"&",
    b"&#65;",
    b"&#x0;",
    b"&nbsp;",
    b"&lt;",
    b"\\\"",
    b"\\0",
    b"\0",
    b"\x01",
    b"\x0B",
    b"\xC3(",
    b"\xff",
    b">",
    b"<",
];

fn piece() -> impl Strategy<Value = Vec<u8>> {
    let text = "[a-z &<>;#\"'/=\\\\\n\t]{0,8}".prop_map(String::into_bytes);
    let quote = prop_oneof![Just(""), Just("\""), Just("'")];
    let attr = (
        prop::sample::select(ATTRS),
        prop::sample::select(VALUES),
        quote,
        any::<bool>(),
    )
        .prop_map(|(n, v, q, valueless)| {
            if valueless {
                format!(" {n}")
            } else {
                format!(" {n}={q}{v}{q}")
            }
        });
    let tag = (
        prop::sample::select(TAGS),
        prop::collection::vec(attr, 0..4),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(t, attrs, closing, self_closing, unterminated)| {
            let mut s = format!("<{}{t}{}", if closing { "/" } else { "" }, attrs.concat());
            if self_closing {
                s.push_str(" /");
            }
            if !unterminated {
                s.push('>');
            }
            s.into_bytes()
        });
    let special = prop::sample::select(SPECIAL).prop_map(<[u8]>::to_vec);
    prop_oneof![3 => tag, 2 => text, 2 => special]
}

/// Token soup: up to 24 pieces of tags, attributes, values, comments, declarations, entities, control bytes and
/// invalid UTF-8.
pub fn token_soup() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(piece(), 0..24).prop_map(|pieces| pieces.concat())
}
