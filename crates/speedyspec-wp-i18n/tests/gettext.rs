//! The gettext facade: lookups through the controller, then WordPress's filters through the Hook API.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use speedyspec_wp_hooks::{Callback, HookApi, HookName, HookValue, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_i18n::{I18nApi, Translated, TranslationController};
use speedyspec_wp_i18n_files::{ByExtension, StdFileProbe};
use speedyspec_wp_i18n_memory::MemoryTranslationFileCache;

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../speedyspec-wp-i18n-files/tests/data")
        .join(name)
}

fn s(v: &str) -> HookValue {
    HookValue::Str(v.into())
}

type Log = Rc<RefCell<Vec<(String, Vec<HookValue>)>>>;

fn api() -> (I18nApi, Rc<HookApi>) {
    let hooks = Rc::new(HookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let controller = TranslationController::new(
        Rc::new(ByExtension::default()),
        Rc::new(StdFileProbe),
        Rc::new(MemoryTranslationFileCache::default()),
    );
    let controller = Rc::new(RefCell::new(controller));
    assert!(
        controller
            .borrow_mut()
            .load_file(&data("simple.mo"), "dom", None)
    );
    assert!(
        controller
            .borrow_mut()
            .load_file(&data("plural.mo"), "dom", None)
    );
    (I18nApi::new(hooks.clone(), controller), hooks)
}

/// What the caller (the native function) would return: its own argument `i` for `Arg(i)`.
fn owned(t: Translated<'_>) -> HookValue {
    match t {
        Translated::Bytes(b) => HookValue::Str(String::from_utf8(b.to_vec()).unwrap()),
        Translated::Arg(i) => HookValue::Str(format!("<arg {i}>")),
        Translated::Value(v) => v,
    }
}

/// Records each call's arguments and appends a marker to the translation.
fn recorder(log: Log, name: &'static str) -> Callback {
    Callback::rust(name, move |args: &[HookValue]| {
        log.borrow_mut().push((name.to_string(), args.to_vec()));
        match &args[0] {
            HookValue::Str(t) => Ok(HookValue::Str(format!("{t}+{name}"))),
            other => Ok(other.clone()),
        }
    })
}

#[test]
fn untranslated_returns_original_without_filters() {
    let (api, _) = api();
    let r = api.translate(b"nothing here", None, "dom", owned).unwrap();
    assert_eq!(r, s("<arg 0>"));
    assert_eq!(
        api.translate(b"baba", None, "dom", owned).unwrap(),
        s("dyado")
    );
}

#[test]
fn no_callbacks_counts_did_filter() {
    let (api, hooks) = api();
    api.translate(b"baba", None, "dom", owned).unwrap();
    api.translate(b"baba", None, "dom", owned).unwrap();
    assert_eq!(hooks.did_filter(&HookName::new(s("gettext"))).unwrap(), 2);
    assert_eq!(
        hooks.did_filter(&HookName::new(s("gettext_dom"))).unwrap(),
        2
    );
}

#[test]
fn filters_fire_in_order_with_wordpress_args() {
    let (api, hooks) = api();
    let log = Rc::new(RefCell::new(Vec::new()));
    hooks
        .add_filter(
            &HookName::new(s("gettext")),
            recorder(log.clone(), "g"),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    hooks
        .add_filter(
            &HookName::new(s("gettext_dom")),
            recorder(log.clone(), "gd"),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    let r = api.translate(b"baba", None, "dom", owned).unwrap();
    assert_eq!(r, s("dyado+g+gd"));
    let log = log.borrow();
    assert_eq!(log[0], ("g".into(), vec![s("dyado"), s("baba"), s("dom")]));
    assert_eq!(
        log[1],
        ("gd".into(), vec![s("dyado+g"), s("baba"), s("dom")])
    );
}

#[test]
fn untranslated_text_is_filtered_as_the_original() {
    let (api, hooks) = api();
    let log = Rc::new(RefCell::new(Vec::new()));
    hooks
        .add_filter(
            &HookName::new(s("gettext")),
            recorder(log.clone(), "g"),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    assert_eq!(
        api.translate(b"nope", None, "dom", owned).unwrap(),
        s("nope+g")
    );
}

#[test]
fn context_filters_get_the_context() {
    let (api, hooks) = api();
    let log = Rc::new(RefCell::new(Vec::new()));
    hooks
        .add_filter(
            &HookName::new(s("gettext_with_context")),
            recorder(log.clone(), "c"),
            &HookValue::Int(10),
            4,
        )
        .unwrap();
    api.translate(b"nope", Some(b"ctx"), "dom", owned).unwrap();
    assert_eq!(
        log.borrow()[0].1,
        vec![s("nope"), s("nope"), s("ctx"), s("dom")]
    );
    assert_eq!(
        api.hooks_for_translate("dom", true)[1].value(),
        &s("gettext_with_context_dom")
    );
}

#[test]
fn plural_picks_the_form_and_filters_with_number() {
    let (api, hooks) = api();
    assert_eq!(
        api.translate_plural(b"one dragon", b"%d dragons", 3, None, "dom", owned)
            .unwrap(),
        s("twoey dragoney")
    );
    // Missing: English rules on the originals.
    assert_eq!(
        api.translate_plural(b"cat", b"cats", 1, None, "dom", owned)
            .unwrap(),
        s("<arg 0>")
    );
    assert_eq!(
        api.translate_plural(b"cat", b"cats", 2, None, "dom", owned)
            .unwrap(),
        s("<arg 1>")
    );
    let log = Rc::new(RefCell::new(Vec::new()));
    hooks
        .add_filter(
            &HookName::new(s("ngettext")),
            recorder(log.clone(), "n"),
            &HookValue::Int(10),
            5,
        )
        .unwrap();
    api.translate_plural(b"cat", b"cats", 2, None, "dom", owned)
        .unwrap();
    assert_eq!(
        log.borrow()[0].1,
        vec![s("cats"), s("cat"), s("cats"), HookValue::Int(2), s("dom")]
    );
}

#[test]
fn filter_may_translate_again() {
    let (api, hooks) = api();
    let api = Rc::new(api);
    let inner = Rc::downgrade(&api);
    let depth = Rc::new(std::cell::Cell::new(0));
    hooks
        .add_filter(
            &HookName::new(s("gettext")),
            Callback::rust("again", move |args: &[HookValue]| {
                if depth.get() > 0 {
                    return Ok(args[0].clone());
                }
                depth.set(1);
                let api = inner.upgrade().unwrap();
                let nested = api
                    .translate(b"baba", None, "dom", owned)
                    .map_err(|e| speedyspec_wp_hooks::HookError::message(e.to_string()))?;
                Ok(HookValue::Str(format!("{:?}|{:?}", args[0], nested)))
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    let r = api.translate(b"baba", None, "dom", owned).unwrap();
    assert!(
        matches!(r, HookValue::Str(ref v) if v.starts_with("Str(\"dyado\")|")),
        "{r:?}"
    );
}

#[test]
fn filters_empty_does_not_count() {
    let (api, hooks) = api();
    assert!(api.filters_empty("dom", false, false));
    assert_eq!(hooks.did_filter(&HookName::new(s("gettext"))).unwrap(), 0);
    let log = Rc::new(RefCell::new(Vec::new()));
    hooks
        .add_filter(
            &HookName::new(s("ngettext_dom")),
            recorder(log.clone(), "n"),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    assert!(!api.filters_empty("dom", false, true));
    assert!(api.filters_empty("dom", true, true));
    hooks
        .add_filter(
            &HookName::new(s("all")),
            recorder(log, "a"),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    assert!(!api.filters_empty("other", false, false));
}

#[test]
fn unhooked_calls_are_served_and_counted_on_the_request_store() {
    let (api, hooks) = api();
    api.set_request_store(Some(hooks.store().clone()));
    assert_eq!(
        api.translate_if_unhooked(b"baba", None, "dom", owned),
        Some(s("dyado"))
    );
    assert_eq!(
        api.translate_if_unhooked(b"nope", None, "dom", owned),
        Some(s("<arg 0>"))
    );
    assert_eq!(
        api.translate_plural_if_unhooked(b"cat", b"cats", 2, None, "dom", owned),
        Some(s("<arg 1>"))
    );
    assert_eq!(hooks.did_filter(&HookName::new(s("gettext"))).unwrap(), 2);
    assert_eq!(
        hooks.did_filter(&HookName::new(s("gettext_dom"))).unwrap(),
        2
    );
    assert_eq!(
        hooks.did_filter(&HookName::new(s("ngettext_dom"))).unwrap(),
        1
    );
    let log = Rc::new(RefCell::new(Vec::new()));
    hooks
        .add_filter(
            &HookName::new(s("gettext_dom")),
            recorder(log, "gd"),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    assert_eq!(api.translate_if_unhooked(b"baba", None, "dom", owned), None);
    assert_eq!(
        hooks.did_filter(&HookName::new(s("gettext"))).unwrap(),
        2,
        "a call it does not serve counts nothing"
    );
}

#[test]
fn count_if_unhooked_counts_only_when_nothing_is_hooked() {
    let (api, hooks) = api();
    assert!(api.count_if_unhooked("dom", false, false));
    assert_eq!(
        hooks.did_filter(&HookName::new(s("gettext_dom"))).unwrap(),
        1
    );
    let log = Rc::new(RefCell::new(Vec::new()));
    hooks
        .add_filter(
            &HookName::new(s("gettext")),
            recorder(log, "g"),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    assert!(!api.count_if_unhooked("dom", false, false));
    assert_eq!(
        hooks.did_filter(&HookName::new(s("gettext_dom"))).unwrap(),
        1
    );
}

#[test]
fn empty_context_still_uses_the_context_filters() {
    let (api, hooks) = api();
    api.translate(b"baba", Some(b""), "dom", owned).unwrap();
    assert_eq!(
        hooks
            .did_filter(&HookName::new(s("gettext_with_context")))
            .unwrap(),
        1
    );
    assert_eq!(hooks.did_filter(&HookName::new(s("gettext"))).unwrap(), 0);
    let log = Rc::new(RefCell::new(Vec::new()));
    hooks
        .add_filter(
            &HookName::new(s("gettext_with_context")),
            recorder(log.clone(), "c"),
            &HookValue::Int(10),
            4,
        )
        .unwrap();
    assert_eq!(
        api.translate_if_unhooked(b"baba", Some(b""), "dom", owned),
        None
    );
    assert_eq!(
        api.translate_plural_if_unhooked(b"cat", b"cats", 2, Some(b""), "dom", owned),
        Some(s("<arg 1>"))
    );
}

#[test]
fn locale_strings_translate_the_default_domain_through_the_filters() {
    use speedyspec_wp_i18n_contracts::LocaleStrings;
    let (api, hooks) = api();
    assert!(
        api.controller()
            .borrow_mut()
            .load_file(&data("simple.mo"), "default", None)
    );
    let log = Rc::new(RefCell::new(Vec::new()));
    hooks
        .add_filter(
            &HookName::new(s("gettext")),
            recorder(log.clone(), "g"),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    assert_eq!(
        LocaleStrings::translate(&api, "baba").unwrap(),
        s("dyado+g")
    );
    assert_eq!(
        LocaleStrings::translate_with_context(&api, "ltr", "text direction").unwrap(),
        s("ltr")
    );
    assert_eq!(
        log.borrow()[0],
        ("g".into(), vec![s("dyado"), s("baba"), s("default")])
    );
}
