//! PHP's `preg_*` semantics over PCRE2, as WordPress's patterns need them.

use speedyspec_wp_text::preg::{Pattern, SplitFlags, preg_match, preg_replace, preg_split};

#[test]
fn php_delimiters_and_modifiers() {
    let p = Pattern::php("@<(script|style)[^>]*?>.*?</\\1>@si");
    let m = preg_match(&p, b"<SCRIPT a>\nx</script>").unwrap().expect("matches");
    assert_eq!(m.full(), b"<SCRIPT a>\nx</script>");
    assert_eq!(m.get(1), Some(&b"SCRIPT"[..]));
    let _ = Pattern::php("|<br\\s*/?>|");
    let _ = Pattern::php("{a}");
}

#[test]
fn replace_references() {
    let p = Pattern::php("/(a)(b)?/");
    let (out, count) = preg_replace(&p, b"xay", b"[$1|${2}|\\1]", None).unwrap();
    assert_eq!(&*out, b"x[a||a]y");
    assert_eq!(count, 1);
    let (out, _) = preg_replace(&p, b"a", b"<$10>", None).unwrap();
    assert_eq!(&*out, b"<>");
    let (out, count) = preg_replace(&p, b"zzz", b"q", None).unwrap();
    assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
    assert_eq!(count, 0);
}

#[test]
fn split_flags() {
    let p = Pattern::php("/(-)/");
    let all = SplitFlags { delim_capture: true, no_empty: true };
    assert_eq!(preg_split(&p, b"a--b", -1, all).unwrap(), vec![&b"a"[..], b"-", b"-", b"b"]);
    let plain = Pattern::php("/-/");
    assert_eq!(preg_split(&plain, b"a--b", -1, SplitFlags::default()).unwrap(), vec![&b"a"[..], b"", b"b"]);
    assert_eq!(preg_split(&plain, b"a-b-c", 2, SplitFlags::default()).unwrap(), vec![&b"a"[..], b"b-c"]);
}

#[test]
fn byte_mode() {
    let p = Pattern::php("/\\xC2\\xA0/");
    assert!(preg_match(&p, b"a\xC2\xA0\xC0").unwrap().is_some());
    let u = Pattern::php("/./u");
    assert!(preg_match(&u, b"a\xC2\xA0\xC0").is_err());
}

#[test]
fn conditional_and_lookbehind() {
    let p = Pattern::php("/\\b(\\d(?(?<=0)[\\d\\.,]+|[\\d\\.,]*))x(\\d[\\d\\.,]*)\\b/");
    let (out, _) = preg_replace(&p, b"9x9 0x99 0.5x2", b"$1&#215;$2", None).unwrap();
    assert_eq!(&*out, b"9&#215;9 0x99 0.5&#215;2");
}
