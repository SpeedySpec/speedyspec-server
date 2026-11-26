//! `make_clickable`, the rel callbacks and `links_add_*`: the recorded corpus through `TextApi`, and their filters.

mod common;

use common::text;
use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue};
use speedyspec_wp_text::links::split_str_by_whitespace;
use speedyspec_wp_text_corpus::{Php, assert_corpus, assert_corpus_reporting, diagnostics};

#[path = "common/api.rs"]
mod api;

fn add(h: &api::Harness, hook: &str, run: impl Fn(&[HookValue]) -> HookValue + 'static) {
    h.hooks
        .add_filter(&HookName::from(hook), Callback::rust(hook, move |args: &[HookValue]| Ok(run(args))), &HookValue::Int(10), 4)
        .unwrap();
}

/// A recorded list of strings (or one string), as `(array)` makes it.
fn list(arg: Option<&Php>, default: &[&str]) -> Vec<Vec<u8>> {
    match arg {
        None => default.iter().map(|s| s.as_bytes().to_vec()).collect(),
        Some(Php::Array(items)) => items.iter().filter_map(|(_, v)| v.bytes().map(<[u8]>::to_vec)).collect(),
        Some(one) => vec![text(one).to_vec()],
    }
}

#[test]
fn link_corpora_through_text_api() {
    let h = api::harness();
    let api = &h.api;
    assert_corpus("make_clickable", |args| Php::Str(api.make_clickable(text(&args[0])).unwrap()));
    assert_corpus("wp_rel_nofollow", |args| Php::Str(api.wp_rel_nofollow(text(&args[0])).unwrap()));
    assert_corpus("wp_rel_ugc", |args| Php::Str(api.wp_rel_ugc(text(&args[0])).unwrap()));
    assert_corpus("links_add_base_url", |args| {
        let attrs = list(args.get(2), &["src", "href"]);
        let attrs: Vec<&[u8]> = attrs.iter().map(Vec::as_slice).collect();
        Php::Str(api.links_add_base_url(text(&args[0]), text(&args[1]), &attrs).unwrap())
    });
    assert_corpus("links_add_target", |args| {
        let tags = list(args.get(2), &["a"]);
        let tags: Vec<&[u8]> = tags.iter().map(Vec::as_slice).collect();
        let target = args.get(1).map_or(&b"_blank"[..], text);
        Php::Str(api.links_add_target(text(&args[0]), target, &tags).unwrap())
    });
    assert_corpus_reporting("wp_targeted_link_rel", |args| {
        h.diagnostics.taken();
        let out = Php::Str(api.wp_targeted_link_rel(text(&args[0])).unwrap());
        (out, diagnostics(&h.diagnostics.taken()))
    });
}

#[test]
fn clickable_inside_comment_text() {
    let h = api::harness();
    let api = h.api.clone();
    h.hooks
        .add_filter(
            &HookName::from("comment_text"),
            Callback::rust("make_clickable", move |args: &[HookValue]| {
                let HookValue::Str(t) = &args[0] else { panic!("a string") };
                Ok(speedyspec_wp_text::strings::bytes_value(api.make_clickable(t.as_bytes()).unwrap()))
            }),
            &HookValue::Int(9),
            1,
        )
        .unwrap();
    assert_corpus("speedyspec_make_clickable_in_comment_text", |args| {
        let out = h.hooks.apply_filters(&HookName::from("comment_text"), HookValue::Str(String::from_utf8(text(&args[0]).to_vec()).unwrap()), Vec::new()).unwrap();
        speedyspec_wp_text_corpus::from_value(&out.read())
    });
}

#[test]
fn clickable_rel_filter_empty_drops_attribute() {
    let h = api::harness();
    add(&h, "make_clickable_rel", |_| HookValue::Str(String::new()));
    assert_eq!(h.api.make_clickable(b"see http://wordpress.org now").unwrap(), b"see <a href=\"http://wordpress.org\">http://wordpress.org</a> now");
}

#[test]
fn clickable_rel_filter_gets_rel_and_url() {
    let h = api::harness();
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let log = seen.clone();
    add(&h, "make_clickable_rel", move |args| {
        log.borrow_mut().push(args.to_vec());
        args[0].clone()
    });
    h.api.make_clickable(b"x http://wordpress.org y").unwrap();
    assert_eq!(seen.borrow()[0], vec![HookValue::Str("nofollow".into()), HookValue::Str("http://wordpress.org".into())]);
}

#[test]
fn targeted_rel_false_leaves_link() {
    let h = api::harness();
    add(&h, "wp_targeted_link_rel", |_| HookValue::Bool(false));
    let link = br#"<a href="http://a" target="_blank">x</a>"#;
    assert_eq!(h.api.wp_targeted_link_rel(link).unwrap(), link);
}

#[test]
fn split_str_by_whitespace_goal() {
    assert_eq!(split_str_by_whitespace(b"aaa bbb ccc", 4), vec![&b"aaa "[..], b"bbb ", b"ccc"]);
    assert_eq!(split_str_by_whitespace(b"aaaaaaaa", 4), vec![&b"aaaaaaaa"[..]]);
}
