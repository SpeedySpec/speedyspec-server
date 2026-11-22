//! `parse_url`, `wp_parse_url` and `parse_str`, against the recorded corpus and PHP's own edge cases.

mod common;

use common::php_int;
use speedyspec_wp_text::parse_str::parse_str;
use speedyspec_wp_text::strings::to_php_string;
use speedyspec_wp_text::url::{UrlParts, parse_url, wp_parse_url};
use speedyspec_wp_text_corpus::{Php, PhpKey, assert_corpus, from_value, to_value};

/// `parse_url()`'s array, in PHP's key order.
fn parts_value(parts: &UrlParts) -> Php {
    let mut out = Vec::new();
    let mut put = |key: &str, value: Php| out.push((PhpKey::Str(key.as_bytes().to_vec()), value));
    let text = |v: &Option<Vec<u8>>| v.clone().map(Php::Str);
    if let Some(v) = text(&parts.scheme) {
        put("scheme", v);
    }
    if let Some(v) = text(&parts.host) {
        put("host", v);
    }
    if let Some(port) = parts.port {
        put("port", Php::Int(i64::from(port)));
    }
    if let Some(v) = text(&parts.user) {
        put("user", v);
    }
    if let Some(v) = text(&parts.pass) {
        put("pass", v);
    }
    if let Some(v) = text(&parts.path) {
        put("path", v);
    }
    if let Some(v) = text(&parts.query) {
        put("query", v);
    }
    if let Some(v) = text(&parts.fragment) {
        put("fragment", v);
    }
    Php::Array(out)
}

/// One component, as `wp_parse_url( $url, $component )` returns it.
fn component(parts: &UrlParts, which: i64) -> Php {
    let text = |v: &Option<Vec<u8>>| v.clone().map_or(Php::Null, Php::Str);
    match which {
        0 => text(&parts.scheme),
        1 => text(&parts.host),
        2 => parts.port.map_or(Php::Null, |p| Php::Int(i64::from(p))),
        3 => text(&parts.user),
        4 => text(&parts.pass),
        5 => text(&parts.path),
        6 => text(&parts.query),
        7 => text(&parts.fragment),
        _ => Php::Null,
    }
}

#[test]
fn wp_parse_url_matches_php() {
    assert_corpus("wp_parse_url", |args| {
        let url = args[0].bytes().expect("a string URL");
        match wp_parse_url(url) {
            None => Php::Bool(false),
            Some(parts) if args.len() > 1 => component(&parts, php_int(&args[1])),
            Some(parts) => parts_value(&parts),
        }
    });
}

#[test]
fn parse_url_rejects_what_php_rejects() {
    for url in [&b"http:///a"[..], b"//:", b"http://a:65536/"] {
        assert!(parse_url(url).is_none(), "{}", url.escape_ascii());
    }
    assert_eq!(parse_url(b"x:").expect("a scheme alone").scheme.as_deref(), Some(&b"x"[..]));
    // PHP reads the unterminated IPv6 host as host "[:" and port 1.
    let broken = parse_url(b"http://[::1").expect("parses");
    assert_eq!((broken.host.as_deref(), broken.port), (Some(&b"[:"[..]), Some(1)));
}

#[test]
fn parse_url_replaces_control_characters() {
    let parts = parse_url(b"http://a\x01b/p\x7fq").expect("parses");
    assert_eq!(parts.host.as_deref(), Some(&b"a_b"[..]));
    assert_eq!(parts.path.as_deref(), Some(&b"/p_q"[..]));
}

#[test]
fn wp_parse_url_relative() {
    let parts = wp_parse_url(b"//example.org/a").expect("parses");
    assert_eq!((parts.scheme, parts.host.as_deref(), parts.path.as_deref()), (None, Some(&b"example.org"[..]), Some(&b"/a"[..])));
    let parts = wp_parse_url(b"/a?b").expect("parses");
    assert_eq!((parts.host, parts.path.as_deref(), parts.query.as_deref()), (None, Some(&b"/a"[..]), Some(&b"b"[..])));
}

#[test]
fn parse_str_matches_php() {
    assert_corpus("speedyspec_wp_parse_str", |args| {
        let input = to_php_string(&to_value(&args[0])).expect("a scalar");
        from_value(&parse_str(&input))
    });
}

#[test]
fn parse_str_limits() {
    let many = vec![&b"a[]=1"[..]; 1001].join(&b'&');
    let Php::Array(top) = from_value(&parse_str(&many)) else { panic!("an array") };
    let Php::Array(list) = &top[0].1 else { panic!("a list") };
    assert_eq!(list.len(), 1000);
    let mut deep = b"a".to_vec();
    for _ in 0..65 {
        deep.extend_from_slice(b"[b]");
    }
    deep.extend_from_slice(b"=1&c=2");
    assert_eq!(from_value(&parse_str(&deep)), Php::Array(vec![(PhpKey::Str(b"c".to_vec()), Php::Str(b"2".to_vec()))]));
}

#[test]
fn parse_str_keys() {
    let parsed = from_value(&parse_str(b" a.b c[=1&n[1]=a&n[x]=b&n[]=c&n[-3]=d&n[05]=e&%5Fz=f&x=1%00y"));
    let s = |v: &str| Php::Str(v.as_bytes().to_vec());
    let k = |v: &str| PhpKey::Str(v.as_bytes().to_vec());
    assert_eq!(
        parsed,
        Php::Array(vec![
            (k("a_b_c_"), s("1")),
            (
                k("n"),
                Php::Array(vec![
                    (PhpKey::Int(1), s("a")),
                    (k("x"), s("b")),
                    (PhpKey::Int(2), s("c")),
                    (PhpKey::Int(-3), s("d")),
                    (k("05"), s("e")),
                ])
            ),
            (k("_z"), s("f")),
            (k("x"), Php::Str(b"1\0y".to_vec())),
        ])
    );
}
