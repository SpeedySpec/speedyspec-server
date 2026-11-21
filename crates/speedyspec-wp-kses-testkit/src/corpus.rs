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
