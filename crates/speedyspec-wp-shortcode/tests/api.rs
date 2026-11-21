//! `ShortcodeApi` against WordPress: recorded cases from `tests/phpunit/tests/shortcode.php` and its data, plus
//! the behaviours the recordings cannot show (filters' arguments, re-entrancy, notices).

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_wp_shortcode_contracts::{
    ContextGuard, PhpKey, PhpValue, ShortcodeError, ShortcodeFilters, ShortcodeRegistry, php_bytes,
};
use support::api_cases::api_cases;
use support::api_types::{Expected, Op};
use support::doubles::{Uncallable, handler, harness, harness_with, register_wordpress_handlers};

fn show(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn wordpress_cases_match() {
    for case in api_cases() {
        let h = harness();
        register_wordpress_handlers(&h);
        let got = match &case.op {
            Op::Do => Expected::Bytes(h.api.do_shortcode(case.input, false).unwrap()),
            Op::DoIgnoreHtml => Expected::Bytes(h.api.do_shortcode(case.input, true).unwrap()),
            Op::Strip => Expected::Bytes(h.api.strip_shortcodes(case.input).unwrap()),
            Op::Has(tag) => {
                h.api.add_shortcode(b"foo", handler(|_, _, _| Ok(PhpValue::Bool(false))));
                Expected::Bool(h.api.has_shortcode(case.input, tag).unwrap())
            }
            Op::Tags => Expected::Tags(h.api.get_shortcode_tags_in_content(case.input).unwrap()),
            Op::TagsEmptyRegistry => {
                h.api.remove_all_shortcodes();
                Expected::Tags(h.api.get_shortcode_tags_in_content(case.input).unwrap())
            }
        };
        if let (Expected::Bytes(g), Expected::Bytes(e)) = (&got, &case.expected) {
            assert_eq!(show(g), show(e), "input {:?}", show(case.input));
        }
        assert_eq!(got, case.expected, "input {:?}", show(case.input));
        assert_eq!(*h.diagnostics.0.borrow(), case.diagnostics, "notices for {:?}", show(case.input));
    }
}

#[test]
fn registration_follows_add_shortcode() {
    let h = harness();
    for bad in ["<html>", "[shortcode]", "bad/", "/bad", "bad space", "&amp;", ""] {
        h.api.add_shortcode(bad.as_bytes(), handler(|_, _, _| Ok(PhpValue::Null)));
        assert!(!h.api.shortcode_exists(bad.as_bytes()), "{bad:?}");
    }
    for good in ["good!", "plain", "unreserved!#$%()*+,-.;?@^_{|}~chars"] {
        h.api.add_shortcode(good.as_bytes(), handler(|_, _, _| Ok(PhpValue::Null)));
        assert!(h.api.shortcode_exists(good.as_bytes()), "{good:?}");
    }
    let notices = h.diagnostics.0.borrow();
    assert_eq!(notices.len(), 7);
    assert_eq!(show(&notices[0]), "doing_it_wrong:add_shortcode|Invalid shortcode name: <html>. Do not use spaces or reserved characters: & / < > [ ] =|4.4.0");
    assert_eq!(show(&notices[6]), "doing_it_wrong:add_shortcode|Invalid shortcode name: Empty name given.|4.4.0");
}

#[test]
fn handler_reenters() {
    let h = harness();
    let weak = Rc::downgrade(&h.api);
    h.api.add_shortcode(b"inner", handler(|_, _, _| Ok(PhpValue::Str("I".into()))));
    h.api.add_shortcode(
        b"outer",
        handler(move |_, content, _| {
            let api = weak.upgrade().unwrap();
            api.remove_shortcode(b"outer");
            Ok(php_bytes(api.do_shortcode(content.unwrap(), false)?))
        }),
    );
    assert_eq!(show(&h.api.do_shortcode(b"[outer][inner][/outer] [outer]", false).unwrap()), "I [outer]");
}

#[test]
fn tag_set_effect() {
    let h = harness();
    h.api.add_shortcode(b"a", handler(|_, _, _| Ok(PhpValue::Str("A".into()))));
    assert_eq!(show(&h.api.do_shortcode(b"[a] [a.x]", false).unwrap()), "A A");
    assert_eq!(show(&h.api.do_shortcode(b"[a.x]", false).unwrap()), "[a.x]");
}

#[test]
fn uncallable_handler_doing_it_wrong() {
    let h = harness();
    h.registry.add(speedyspec_wp_shortcode_contracts::TagName::new(b"gone").unwrap(), Rc::new(Uncallable));
    assert_eq!(show(&h.api.do_shortcode(b"x [gone a=1] y", false).unwrap()), "x [gone a=1] y");
    assert_eq!(
        show(&h.diagnostics.0.borrow()[0]),
        "doing_it_wrong:do_shortcode_tag|Attempting to parse a shortcode without a valid callback: gone|4.3.0"
    );
}

#[test]
fn invalid_utf8_content_passes_through() {
    let h = harness();
    h.api.add_shortcode(b"a", handler(|_, _, _| Ok(PhpValue::Str("A".into()))));
    assert_eq!(h.api.do_shortcode(b"\xff[a]\xfe", false).unwrap(), b"\xffA\xfe".to_vec());
}

#[test]
fn invalid_utf8_atts_report_deprecation() {
    let h = harness();
    assert!(h.api.shortcode_parse_atts(b"\xff a=1").is_empty());
    assert_eq!(show(&h.diagnostics.0.borrow()[0]), "deprecated_null_subject:preg_match_all");
}

type TagFilter = Box<dyn Fn(PhpValue) -> PhpValue>;
type AttsFilter = Box<dyn Fn(PhpValue, &PhpValue) -> PhpValue>;

/// Filters scripted per test, recording what WordPress would pass them.
#[derive(Default)]
struct Scripted {
    pre: Option<PhpValue>,
    tag: Option<TagFilter>,
    atts: Option<AttsFilter>,
    tagnames: Option<PhpValue>,
    seen: RefCell<Vec<(String, PhpValue)>>,
    entered: RefCell<Vec<&'static str>>,
}

impl ShortcodeFilters for Scripted {
    fn pre_do_shortcode_tag(&self, tag: &[u8], attr: &PhpValue, m: &PhpValue) -> Result<PhpValue, ShortcodeError> {
        self.seen.borrow_mut().push(("pre".into(), PhpValue::Array(vec![
            (PhpKey::Str("key".into()), php_bytes(tag.to_vec())),
            (PhpKey::Str("atts".into()), attr.clone()),
            (PhpKey::Str("m".into()), m.clone()),
        ])));
        Ok(self.pre.clone().unwrap_or(PhpValue::Bool(false)))
    }

    fn do_shortcode_tag(&self, output: PhpValue, tag: &[u8], attr: &PhpValue, m: &PhpValue) -> Result<PhpValue, ShortcodeError> {
        self.seen.borrow_mut().push(("tag".into(), PhpValue::Array(vec![
            (PhpKey::Str("output".into()), output.clone()),
            (PhpKey::Str("key".into()), php_bytes(tag.to_vec())),
            (PhpKey::Str("atts".into()), attr.clone()),
            (PhpKey::Str("m".into()), m.clone()),
        ])));
        Ok(match &self.tag {
            Some(f) => f(output),
            None => output,
        })
    }

    fn shortcode_atts(&self, shortcode: &[u8], out: PhpValue, pairs: &PhpValue, atts: &PhpValue) -> Result<PhpValue, ShortcodeError> {
        self.seen.borrow_mut().push((format!("atts_{}", show(shortcode)), PhpValue::Array(vec![
            (PhpKey::Str("out".into()), out.clone()),
            (PhpKey::Str("pairs".into()), pairs.clone()),
            (PhpKey::Str("atts".into()), atts.clone()),
        ])));
        Ok(match &self.atts {
            Some(f) => f(out, pairs),
            None => out,
        })
    }

    fn strip_shortcodes_tagnames(&self, tags: PhpValue, _: &[u8]) -> Result<PhpValue, ShortcodeError> {
        Ok(self.tagnames.clone().unwrap_or(tags))
    }

    fn enter_do_shortcode_context(&self) -> Result<ContextGuard, ShortcodeError> {
        self.entered.borrow_mut().push("enter");
        Ok(ContextGuard::new(true))
    }

    fn leave_do_shortcode_context(&self, _: ContextGuard) -> Result<(), ShortcodeError> {
        self.entered.borrow_mut().push("leave");
        Ok(())
    }
}

fn s(v: &str) -> PhpValue {
    PhpValue::Str(v.into())
}

fn m(tag: &str) -> PhpValue {
    let whole = format!("[{tag} a='b' c='d']");
    PhpValue::Array(
        [whole.as_str(), "", tag, " a='b' c='d'", "", "", ""].iter().enumerate().map(|(i, g)| (PhpKey::Int(i as i64), s(g))).collect(),
    )
}

fn ab_cd() -> PhpValue {
    PhpValue::Array(vec![(PhpKey::Str("a".into()), s("b")), (PhpKey::Str("c".into()), s("d"))])
}

#[test]
fn pre_do_shortcode_tag_short_circuits_with_wordpress_arguments() {
    let filters = Rc::new(Scripted { pre: Some(s("bar")), ..Default::default() });
    let h = harness_with(filters.clone());
    h.api.add_shortcode(b"pre_do_shortcode_tag", handler(|_, _, _| Ok(s("foo"))));
    assert_eq!(show(&h.api.do_shortcode(b"[pre_do_shortcode_tag a='b' c='d']", false).unwrap()), "bar");
    let seen = filters.seen.borrow();
    assert_eq!(seen.len(), 1, "the handler and do_shortcode_tag are skipped");
    assert_eq!(seen[0].1, PhpValue::Array(vec![
        (PhpKey::Str("key".into()), s("pre_do_shortcode_tag")),
        (PhpKey::Str("atts".into()), ab_cd()),
        (PhpKey::Str("m".into()), m("pre_do_shortcode_tag")),
    ]));
}

#[test]
fn do_shortcode_tag_filters_output_with_wordpress_arguments() {
    let filters = Rc::new(Scripted {
        tag: Some(Box::new(|out| match out {
            PhpValue::Str(v) => PhpValue::Str(v.replace("oo", "ee")),
            other => other,
        })),
        ..Default::default()
    });
    let h = harness_with(filters.clone());
    h.api.add_shortcode(b"do_shortcode_tag", handler(|_, _, _| Ok(s("foo"))));
    assert_eq!(show(&h.api.do_shortcode(b"[do_shortcode_tag a='b' c='d']", false).unwrap()), "fee");
    assert_eq!(filters.seen.borrow()[1].1, PhpValue::Array(vec![
        (PhpKey::Str("output".into()), s("foo")),
        (PhpKey::Str("key".into()), s("do_shortcode_tag")),
        (PhpKey::Str("atts".into()), ab_cd()),
        (PhpKey::Str("m".into()), m("do_shortcode_tag")),
    ]));
}

#[test]
fn shortcode_atts_filter_gets_out_pairs_and_atts() {
    let filters = Rc::new(Scripted {
        atts: Some(Box::new(|out, pairs| {
            // filter_atts2: foo1 becomes the default, and baz is removed.
            let PhpValue::Array(entries) = out else { unreachable!() };
            let PhpValue::Array(pairs) = pairs else { unreachable!() };
            let default_foo = pairs[0].1.clone();
            PhpValue::Array(
                entries
                    .into_iter()
                    .filter(|(k, _)| *k != PhpKey::Str("baz".into()))
                    .map(|(k, v)| if k == PhpKey::Str("foo".into()) && v == s("foo1") { (k, default_foo.clone()) } else { (k, v) })
                    .collect(),
            )
        })),
        ..Default::default()
    });
    let h = harness_with(filters.clone());
    register_wordpress_handlers(&h);
    assert_eq!(show(&h.api.do_shortcode(br#"[bartag foo="foo1" baz="baz1" /]"#, false).unwrap()), "foo = no foo");
    assert_eq!(show(&h.api.do_shortcode(br#"[bartag foo="foo2" /]"#, false).unwrap()), "foo = foo2");
    let seen = filters.seen.borrow();
    let (name, args) = seen.iter().find(|(n, _)| n.starts_with("atts_")).unwrap();
    assert_eq!(name, "atts_bartag");
    assert_eq!(*args, PhpValue::Array(vec![
        (PhpKey::Str("out".into()), PhpValue::Array(vec![(PhpKey::Str("foo".into()), s("foo1")), (PhpKey::Str("baz".into()), s("baz1"))])),
        (PhpKey::Str("pairs".into()), PhpValue::Array(vec![(PhpKey::Str("foo".into()), s("no foo")), (PhpKey::Str("baz".into()), s("default baz"))])),
        (PhpKey::Str("atts".into()), PhpValue::Array(vec![(PhpKey::Str("foo".into()), s("foo1")), (PhpKey::Str("baz".into()), s("baz1"))])),
    ]));
}

#[test]
fn shortcode_atts_skips_the_filter_for_a_falsy_name() {
    let filters = Rc::new(Scripted::default());
    let h = harness_with(filters.clone());
    let pairs = PhpValue::Array(vec![(PhpKey::Str("a".into()), s("1"))]);
    let out = h.api.shortcode_atts(&pairs, &PhpValue::Null, b"0").unwrap();
    assert_eq!(out, pairs);
    assert!(filters.seen.borrow().is_empty());
}

#[test]
fn strip_shortcodes_tagnames_filter_narrows_the_tags() {
    let filters = Rc::new(Scripted { tagnames: Some(PhpValue::Array(vec![(PhpKey::Int(0), s("gallery"))])), ..Default::default() });
    let h = harness_with(filters);
    register_wordpress_handlers(&h);
    assert_eq!(show(&h.api.strip_shortcodes(b"before[gallery]middle [footag]after").unwrap()), "beforemiddle [footag]after");
}

#[test]
fn context_filter_stays_on_error() {
    let filters = Rc::new(Scripted::default());
    let h = harness_with(filters.clone());
    h.api.add_shortcode(b"boom", handler(|_, _, _| Err(ShortcodeError::Handler("thrown".into()))));
    assert!(h.api.do_shortcode(b"[boom]", false).is_err());
    assert_eq!(*filters.entered.borrow(), vec!["enter"]);
    h.api.add_shortcode(b"ok", handler(|_, _, _| Ok(s("k"))));
    filters.entered.borrow_mut().clear();
    h.api.do_shortcode(b"[ok]", false).unwrap();
    assert_eq!(*filters.entered.borrow(), vec!["enter", "leave"]);
}

#[test]
fn direct_do_shortcode_tag_returns_filter_values_unconverted() {
    let filters = Rc::new(Scripted { pre: Some(PhpValue::Array(vec![(PhpKey::Int(0), PhpValue::Int(1))])), ..Default::default() });
    let h = harness_with(filters);
    h.api.add_shortcode(b"t", handler(|_, _, _| Ok(s("x"))));
    let m = PhpValue::Array(["[t]", "", "t", "", "", "", ""].iter().enumerate().map(|(i, g)| (PhpKey::Int(i as i64), s(g))).collect());
    assert_eq!(h.api.do_shortcode_tag(&m).unwrap(), PhpValue::Array(vec![(PhpKey::Int(0), PhpValue::Int(1))]));
}

#[test]
fn direct_do_shortcode_tag_without_group_five_passes_null_content() {
    let h = harness();
    h.api.add_shortcode(b"t", handler(|_, content, _| Ok(s(if content.is_none() { "null" } else { "some" }))));
    let short = PhpValue::Array(["[t]", "", "t", ""].iter().enumerate().map(|(i, g)| (PhpKey::Int(i as i64), s(g))).collect());
    assert_eq!(h.api.do_shortcode_tag(&short).unwrap(), s("null"));
}

#[test]
fn get_shortcode_regex_uses_the_registry_when_given_nothing() {
    let h = harness();
    h.api.add_shortcode(b"gallery", handler(|_, _, _| Ok(PhpValue::Null)));
    h.api.add_shortcode(b"a.b", handler(|_, _, _| Ok(PhpValue::Null)));
    let all = h.api.get_shortcode_regex(None);
    assert!(show(&all).starts_with(r"\[(\[?)(gallery|a\.b)(?![\w-])"));
    assert_eq!(h.api.get_shortcode_regex(Some(&[])), all);
    assert!(show(&h.api.get_shortcode_regex(Some(&[b"x".to_vec()]))).starts_with(r"\[(\[?)(x)"));
}
