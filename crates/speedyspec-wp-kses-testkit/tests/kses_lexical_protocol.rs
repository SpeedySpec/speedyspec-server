use std::borrow::Cow;

use speedyspec_wp_kses_contracts::Protocols;
use speedyspec_wp_kses_lexical::bad_protocol;
use speedyspec_wp_kses_testkit::{PhpOracle, bytes, from_bytes, protocols_value, xss_attacks};

fn defaults() -> Protocols {
    Protocols(
        [
            "http", "https", "ftp", "ftps", "mailto", "news", "irc", "irc6", "ircs", "gopher",
            "nntp", "feed", "telnet", "mms", "rtsp", "sms", "svn", "tel", "fax", "xmpp", "webcal",
            "urn",
        ]
        .iter()
        .map(|p| p.as_bytes().into())
        .collect(),
    )
}

#[test]
fn strips_script_schemes() {
    let p = defaults();
    assert_eq!(&*bad_protocol(b"javascript:alert(1)", &p), b"alert(1)");
    assert_eq!(&*bad_protocol(b"JaVaScRiPt:x", &p), b"x");
    assert!(matches!(bad_protocol(b"http://a", &p), Cow::Borrowed(_)));
    assert_eq!(&*bad_protocol(b"/path?x:y", &p), b"y");
    assert_eq!(&*bad_protocol(b"a/?b:c", &p), b"a/?b:c");
    assert_eq!(&*bad_protocol(b"feed:http://a", &p), b"feed:http://a");
}

fn inputs() -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = [
        &b""[..],
        b"0",
        b":",
        b"x:",
        b"http://a",
        b"https://a",
        b"HTTP://a",
        b"ftp://a",
        b"javascript:alert(1)",
        b"jav&#x09;ascript:x",
        b"java\x00script:x",
        b"&#106;avascript:x",
        b"JaVaScRiPt:x",
        b"javascript&colon;x",
        b"javascript&#58x",
        b"javascript&#058;x",
        b"javascript&#x3a;x",
        b"javascript&#X3A;x",
        b"javascript&#x3ax",
        b"javascript&#x003Ax",
        b" javascript:x",
        b"java script:x",
        b"java\tscript:x",
        b"java\nscript:x",
        b"/path?x:y",
        b"a/?b:c",
        b"a?b:c",
        b"a/b:c",
        b"feed:http://a",
        b"feed:feed:feed:javascript:x",
        b"feed:feed:feed:feed:http://a",
        b"feed:0",
        b"feed:",
        b"mailto:a@b",
        b"data:text/html,<x>",
        b"vbscript:x",
        b"javascript:javascript:x",
        b"jajavascript:vascript:x",
        b"&colon;x",
        b"&COLON;x",
        b"x\\0y:z",
        b"x\\00:y",
        b"%6aavascript:x",
        b"\xC3(:x",
        b"urn:x",
        b"tel:+1",
        b"https:x",
        b"http:x",
    ]
    .iter()
    .map(|s| s.to_vec())
    .collect();
    v.extend(xss_attacks().into_iter().flat_map(|a| uri_values(&a)));
    v
}

/// Values of URL-like attributes in an attack string, roughly: what follows `href=`, `src=` and the like.
fn uri_values(s: &[u8]) -> Vec<Vec<u8>> {
    let lower = s.to_ascii_lowercase();
    let mut out = Vec::new();
    for key in [
        &b"href="[..],
        b"src=",
        b"action=",
        b"background=",
        b"dynsrc=",
        b"lowsrc=",
        b"url(",
    ] {
        let mut at = 0;
        while let Some(i) = memchr::memmem::find(&lower[at..], key) {
            let start = at + i + key.len();
            let end = s[start..]
                .iter()
                .position(|b| b" >)".contains(b))
                .map_or(s.len(), |e| start + e);
            out.push(
                s[start..end]
                    .iter()
                    .copied()
                    .filter(|b| *b != b'"' && *b != b'\'')
                    .collect(),
            );
            at = start;
        }
    }
    out
}

#[test]
fn matches_php() {
    let mut php = PhpOracle::start().unwrap();
    let p = defaults();
    for input in inputs() {
        let expected = from_bytes(
            &php.call(
                "wp_kses_bad_protocol",
                vec![bytes(&input), protocols_value(&p)],
            )
            .unwrap()
            .returned,
        );
        assert_eq!(
            &*bad_protocol(&input, &p),
            &expected[..],
            "{:?}",
            String::from_utf8_lossy(&input)
        );
    }
}
