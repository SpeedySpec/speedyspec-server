use speedyspec_router::wordpress::wp_sanitize_redirect;

/// Expected values are WordPress 7.2-RC2's `wp_sanitize_redirect()` on the same input.
#[test]
fn sanitizes_like_wordpress() {
    let cases = [
        ("/new path\r\n%0d%0Aé<x>", "/new%20path%C3%A9x"),
        ("/a%0%0dd", "/a"),
        (
            "https://example.test/ü?q=a b&x=<y>#frag",
            "https://example.test/%C3%BC?q=a%20b&x=y#frag",
        ),
        ("/a\\0b", "/a0b"),
        (
            "/日本語/~ok(1)@[x]!*;,",
            "/%E6%97%A5%E6%9C%AC%E8%AA%9E/~ok(1)@[x]!*;,",
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(wp_sanitize_redirect(input), expected, "{input:?}");
    }
}
