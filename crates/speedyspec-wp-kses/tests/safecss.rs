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

/// `$css_test_string` as PHP builds it: `str_replace( $url_match, '', ... )` for each `url\([^)]+\)` match in turn.
fn php_url_test_string(item: &[u8], value: &[u8]) -> Vec<u8> {
    let mut matches = Vec::new();
    let mut at = 0;
    while let Some(i) = memchr::memmem::find(&value[at..], b"url(") {
        let start = at + i;
        match memchr::memchr(b')', &value[start + 4..]) {
            Some(len) if len > 0 => {
                matches.push(value[start..start + 4 + len + 1].to_vec());
                at = start + 4 + len + 1;
            }
            _ => at = start + 1,
        }
    }
    let mut test = item.to_vec();
    for m in matches {
        let mut out = Vec::new();
        let mut rest = &test[..];
        while let Some(i) = memchr::memmem::find(rest, &m) {
            out.extend_from_slice(&rest[..i]);
            rest = &rest[i + m.len()..];
        }
        out.extend_from_slice(rest);
        test = out;
    }
    test
}

#[test]
fn url_test_string_matches_php_str_replace() {
    use proptest::prelude::*;
    use speedyspec_wp_hooks::HookValue;
    use std::cell::RefCell;
    use std::rc::Rc;
    let pieces = prop::sample::select(vec![
        "url(",
        ")",
        "u",
        "rl(",
        "a",
        "b",
        " ",
        "url(a)",
        "url(b)",
        "rl(b)",
        "url(url(a)",
    ]);
    let value = prop::collection::vec(pieces, 0..12).prop_map(|v| v.concat());
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: 5000,
        failure_persistence: None,
        ..Default::default()
    });
    let compared = std::cell::Cell::new(0usize);
    runner
        .run(&value, |value| {
            let (hooks, api) = common::setup();
            let seen: Rc<RefCell<Option<HookValue>>> = Rc::default();
            let keep = seen.clone();
            common::filter(&hooks, "safecss_filter_attr_allow_css", move |a| {
                *keep.borrow_mut() = a.last().cloned();
                a[0].clone()
            });
            let css = format!("background:{value}");
            api.safecss_filter_attr(css.as_bytes()).unwrap();
            if let Some(HookValue::Str(test)) = seen.borrow().clone() {
                let expected =
                    php_url_test_string(css.trim().as_bytes(), &css.trim().as_bytes()[11..]);
                prop_assert_eq!(test.as_bytes(), &expected[..], "{:?}", css);
                compared.set(compared.get() + 1);
            }
            Ok(())
        })
        .unwrap();
    assert!(
        compared.get() > 200,
        "only {} inputs reached the filter",
        compared.get()
    );
}
