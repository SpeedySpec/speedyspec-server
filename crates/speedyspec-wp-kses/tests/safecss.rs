mod common;

use speedyspec_wp_kses_testkit::{PhpOracle, bytes, css_cases, from_bytes};

#[test]
fn keeps_allowed_properties() {
    let (_, api) = common::setup();
    assert_eq!(
        api.safecss_filter_attr(b"color: red; foo: bar").unwrap(),
        b"color: red"
    );
    assert_eq!(
        api.safecss_filter_attr(b"background: url(javascript:x)")
            .unwrap(),
        b""
    );
    assert_eq!(
        api.safecss_filter_attr(b"width: calc(100% - 2px)").unwrap(),
        b"width: calc(100% - 2px)"
    );
    assert_eq!(
        api.safecss_filter_attr(b"--my-var: 1px").unwrap(),
        b"--my-var: 1px"
    );
}

#[test]
fn matches_php_with_hook_counts() {
    let mut php = PhpOracle::start().unwrap();
    for css in css_cases() {
        let (hooks, api) = common::setup();
        let r = php.call("safecss_filter_attr", vec![bytes(&css)]).unwrap();
        let ours = api.safecss_filter_attr(&css).unwrap();
        assert_eq!(
            ours,
            from_bytes(&r.returned),
            "{:?}",
            String::from_utf8_lossy(&css)
        );
        for (hook, count) in &r.filters {
            assert_eq!(
                common::did_filter(&hooks, hook),
                *count as i64,
                "{hook} for {:?}",
                String::from_utf8_lossy(&css)
            );
        }
    }
}
