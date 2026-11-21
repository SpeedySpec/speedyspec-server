use speedyspec_wp_kses_lexical::scan::*;

#[test]
fn space_is_pcre_non_utf() {
    for b in [b' ', b'\t', b'\n', 0x0B, 0x0C, b'\r'] {
        assert!(is_space(b), "{b:#x}");
    }
    assert!(!is_space(0xA0));
    assert!(!is_space(0));
}

#[test]
fn tokens() {
    assert_eq!(
        next_token(b"a<!--x\ny-->b", 0),
        Some((1, 11, TokenKind::Declaration))
    );
    assert_eq!(
        next_token(b"a<!--x-->b", 0),
        Some((1, 9, TokenKind::Comment))
    );
    assert_eq!(next_token(b"<!--open", 0), Some((0, 8, TokenKind::Comment)));
    assert_eq!(next_token(b"x>y", 0), Some((1, 2, TokenKind::StrayGt)));
    assert_eq!(next_token(b"<a", 0), Some((0, 2, TokenKind::Tag)));
    assert_eq!(
        next_token(b"</1x>", 0),
        Some((0, 5, TokenKind::InvalidCloser))
    );
    assert_eq!(next_token(b"<!x>", 0), Some((0, 4, TokenKind::Declaration)));
    assert_eq!(next_token(b"</1x", 0), Some((0, 4, TokenKind::Tag)));
    assert_eq!(next_token(b"plain", 0), None);
    assert_eq!(next_token(b"<a><b>", 3), Some((3, 6, TokenKind::Tag)));
}

#[test]
fn tag_heads() {
    let h = tag_head(b"< / b >").unwrap();
    assert!(h.slash);
    assert_eq!(&b"< / b >"[h.name.clone()], b"b");
    let h = tag_head(b"<a href=x>").unwrap();
    assert!(!h.slash);
    assert_eq!(&b"<a href=x>"[h.attrs.clone()], b" href=x");
    assert!(tag_head(b"<>").is_none());
    assert!(tag_head(b"<a").is_some());
}

#[test]
fn invalid_wrappers() {
    assert!(invalid_wrapper(b"</1>"));
    assert!(invalid_wrapper(b"<!x y>"));
    assert!(!invalid_wrapper(b"<!X>"));
    assert!(!invalid_wrapper(b"</a>"));
}

#[test]
fn attribute_pieces() {
    assert_eq!(attr_name(b"data-x:y.z=1"), 10);
    assert_eq!(attr_name(b"1a"), 0);
    assert_eq!(equals(b" = x"), 3);
    assert_eq!(leading_space(b"  x"), 2);
    assert!(xhtml_slash(b" a / "));
    assert!(!xhtml_slash(b" a"));
    assert_eq!(trailing_space(b"x  "), 2);
    assert_eq!(next_token(b"<!--x\n", 0), Some((0, 5, TokenKind::Comment)));
    assert_eq!(
        next_token(b"</>>", 0),
        Some((0, 4, TokenKind::InvalidCloser))
    );
    assert!(invalid_wrapper(b"</>>"));
    assert!(tag_head(b"<a>\n").is_some());
}
