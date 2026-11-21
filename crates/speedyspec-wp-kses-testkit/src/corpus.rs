//! Test inputs.

use std::path::Path;

use serde_json::{Value, json};
use speedyspec_wp_kses_contracts::Protocols;

use crate::bytes;

fn corpus_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/corpus"))
}

/// Each `<code>` of WordPress's `xssAttacks.xml`, entities in the XML decoded.
pub fn xss_attacks() -> Vec<Vec<u8>> {
    let xml = std::fs::read(corpus_dir().join("wordpress/xssAttacks.xml")).expect("xssAttacks.xml");
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = memchr::memmem::find(&xml[at..], b"<code>") {
        let start = at + i + 6;
        let Some(len) = memchr::memmem::find(&xml[start..], b"</code>") else {
            break;
        };
        out.push(xml_unescape(&xml[start..start + len]));
        at = start + len;
    }
    out
}

fn xml_unescape(s: &[u8]) -> Vec<u8> {
    let mut text = s.to_vec();
    for (from, to) in [
        (&b"&lt;"[..], &b"<"[..]),
        (b"&gt;", b">"),
        (b"&quot;", b"\""),
        (b"&apos;", b"'"),
        (b"&amp;", b"&"),
    ] {
        text = replace(&text, from, to);
    }
    text
}

fn replace(s: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut at = 0;
    while let Some(i) = memchr::memmem::find(&s[at..], from) {
        out.extend_from_slice(&s[at..at + i]);
        out.extend_from_slice(to);
        at += i + from.len();
    }
    out.extend_from_slice(&s[at..]);
    out
}

/// A protocol list as a PHP list for the oracle.
pub fn protocols_value(p: &Protocols) -> Value {
    json!({ "array": p.0.iter().enumerate().map(|(i, v)| json!([i, bytes(v)])).collect::<Vec<_>>() })
}

/// CSS for `safecss_filter_attr()`: WordPress's own test cases plus edge cases of each rule it applies.
pub fn css_cases() -> Vec<Vec<u8>> {
    use base64::Engine;
    let text =
        std::fs::read_to_string(corpus_dir().join("css/wordpress-cases.b64")).expect("css cases");
    let mut cases: Vec<Vec<u8>> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<String>(l).ok())
        .filter_map(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
        .collect();
    for extra in [
        &b"color: red; foo: bar"[..],
        b"  ;;color:red;;  ",
        b"color",
        b":red",
        b"COLOR: red",
        b" color : red ",
        b"background: url(x.png)",
        b"background: url('x.png')",
        b"background: url(\"x.png\")",
        b"background: url( 'x.png' )",
        b"background: url('x.png)",
        b"background: url()",
        b"background: url(0)",
        b"background: url( )",
        b"background: url(javascript:alert(1))",
        b"background: url(http://a/b.png) url(c.png)",
        b"background: url(a.png)x(",
        b"background-image: linear-gradient(red, blue)",
        b"background: repeating-radial-gradient(circle, red 10%, rgb(1,2,3) 20%)",
        b"background: linear-gradient(red, rgba(0,0,0,calc(1 - .5)))",
        b"background: conic-gradient(red, blue); color: red",
        b"width: calc(100% - 2px)",
        b"width: calc(100% - (2px * 3))",
        b"width: min(1px, max(2px, 3px))",
        b"width: calc(1px",
        b"width: CALC(1px)",
        b"transform: rotateX(10deg) translate3d(1px,2px,3px)",
        b"transform: matrix3d(1,2)",
        b"width: varx(1px)",
        b"width: xvar(1px)",
        b"width: var(--a)",
        b"--a: 1px",
        b"--a_b: url(x.png)",
        b"--a: url(javascript:x)",
        b"--a: linear-gradient(red, blue)",
        b"--: 1px",
        b"--a b: 1px",
        b"color: red\\",
        b"color: red}",
        b"color: a=b",
        b"color: a&b",
        b"color: /* x */ red",
        b"color: red\n; width: 1px",
        b"color:\tred",
        b"col\x00or: red",
        b"color: red;; ;width: 1px",
        b"clip-path: polygon(0 0, 100% 0)",
        b"filter: url(#f)",
        b"cursor: url(a.cur), auto",
        b"mask: url(a.svg#m)",
        b"background: url(a.png) no-repeat, url(b.png)",
        b"background: url(  \"a.png\"  )",
        b"background: url(\"a\"b\")",
        b"background: url(feed:javascript:x)",
        b"x: url(a.png)",
        b"display: none",
        b"position: fixed; z-index: 9999",
    ] {
        cases.push(extra.to_vec());
    }
    cases
}

/// WordPress's `whole-posts.php` posts and the block fixtures: representative post content.
pub fn posts() -> Vec<Vec<u8>> {
    use base64::Engine;
    let dir = corpus_dir().join("wordpress");
    let json: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(dir.join("whole-posts.json")).expect("whole-posts.json"),
    )
    .expect("json");
    let mut posts: Vec<Vec<u8>> = json
        .iter()
        .filter_map(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
        .collect();
    let mut blocks: Vec<_> = std::fs::read_dir(dir.join("blocks"))
        .expect("blocks")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    blocks.sort();
    posts.extend(blocks.iter().filter_map(|p| std::fs::read(p).ok()));
    posts
}

fn b64_list(path: &Path) -> Vec<Vec<u8>> {
    use base64::Engine;
    let json: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
    )
    .expect("a JSON list of base64 strings");
    json.iter()
        .filter_map(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
        .collect()
}

/// Inputs whose bytes are not valid UTF-8, or hold NUL and control bytes, in markup and text.
fn invalid_bytes() -> Vec<Vec<u8>> {
    [
        &b"a\xC3(b"[..],
        b"\x80<p>x</p>",
        b"<p title=\"\xff\">x</p>",
        b"<a href=\"\xC0\xAFjavascript:x\">y</a>",
        b"<\xE2\x80p>x",
        b"x\x00y<b\x00>z</b\x00>",
        b"<p>\\0\\00\\\\0</p>",
        b"<a href=\"java\x00script:x\">y</a>",
        b"<p\x01 class=x>y</p>",
        b"<img src=x\x0Bonerror=alert(1)>",
        b"<p>\xF0\x9F\x92\xA9</p>",
        b"<p title=\"\xF0\x9F\x92\">x</p>",
        b"\xEF\xBB\xBF<p>bom</p>",
        b"<a title=\"\xED\xA0\x80\">surrogate</a>",
        b"<p>\r\n\r</p>",
        b"&#0;&#x0;&#xD800;&#xFFFE;",
    ]
    .iter()
    .map(|s| s.to_vec())
    .collect()
}

/// A named set of test inputs: `whole-posts`, `block-fixtures`, `xss-attacks`, `html-api-inputs`, `html5lib`,
/// `invalid-bytes`, and later `attacks` and `bombs`.
pub fn corpus(name: &str) -> Vec<Vec<u8>> {
    let dir = corpus_dir();
    match name {
        "whole-posts" => b64_list(&dir.join("wordpress/whole-posts.json")),
        "block-fixtures" => {
            posts().split_off(b64_list(&dir.join("wordpress/whole-posts.json")).len())
        }
        "xss-attacks" => xss_attacks(),
        "html-api-inputs" => b64_list(&dir.join("wordpress/html-api-inputs.json")),
        "html5lib" => b64_list(&dir.join("html5lib/tree-construction-data.json")),
        "invalid-bytes" => invalid_bytes(),
        other => panic!("no corpus set named {other}"),
    }
}

/// Every corpus set's name.
pub const CORPUS_SETS: &[&str] = &[
    "whole-posts",
    "block-fixtures",
    "xss-attacks",
    "html-api-inputs",
    "html5lib",
    "invalid-bytes",
];
