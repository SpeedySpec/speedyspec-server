use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_wp_hooks::{Callback, HookApi, HookName, HookValue, IgnoreDeprecations, Key};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_options::{
    FixedProtectedOptions, HookSanitizing, Installing, InstallingFlag, OptionName, OptionsError,
    ProtectedOptions, RustCodec, Sanitizing, ValueCodec,
};

fn s(v: &str) -> HookValue {
    HookValue::Str(v.into())
}

fn hooks() -> Rc<HookApi> {
    Rc::new(HookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ))
}

fn name(n: &str) -> OptionName {
    OptionName::new(n).unwrap()
}

#[test]
fn hook_sanitizing_fires_named_filter() {
    let hooks = hooks();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let saw = seen.clone();
    hooks
        .add_filter(
            &HookName::from("sanitize_option_my_opt"),
            Callback::rust("sanitize", move |args: &[HookValue]| {
                saw.borrow_mut().extend_from_slice(args);
                Ok(s("clean"))
            }),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    let got = HookSanitizing::new(hooks)
        .sanitize_option(&name("my_opt"), s("v"))
        .unwrap();
    assert_eq!(got, s("clean"));
    assert_eq!(*seen.borrow(), vec![s("v"), s("my_opt"), s("v")]);
}

#[test]
fn hook_sanitizing_refuses_core_names() {
    let hooks = hooks();
    let fired = Rc::new(RefCell::new(false));
    let f = fired.clone();
    hooks
        .add_filter(
            &HookName::from("sanitize_option_blogname"),
            Callback::rust("never", move |args: &[HookValue]| {
                *f.borrow_mut() = true;
                Ok(args[0].clone())
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    let got = HookSanitizing::new(hooks).sanitize_option(&name("blogname"), s("x"));
    assert!(matches!(
        got,
        Err(OptionsError::NeedsPhp { function: "sanitize_option", ref option }) if option == "blogname"
    ));
    assert!(!*fired.borrow());
}

#[test]
fn protected_names() {
    let p = FixedProtectedOptions;
    for n in ["alloptions", "notoptions"] {
        match p.protect(n) {
            Err(OptionsError::Protected(m)) => {
                assert_eq!(
                    m,
                    format!("{n} is a protected WP option and may not be modified")
                )
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(p.protect("Alloptions").is_ok());
}

#[test]
fn installing_flag_toggles() {
    let flag = InstallingFlag::new();
    assert!(!flag.installing());
    flag.set(true);
    assert!(flag.installing());
    assert!(!flag.setup_config());
    flag.set(false);
    assert!(!flag.installing());
}

#[test]
fn rust_codec_round_trips() {
    let c = RustCodec;
    let array = HookValue::Array(vec![(Key::Str("a".into()), HookValue::Int(1))]);
    let stored = c.maybe_serialize(&array).unwrap();
    assert_eq!(stored, s("a:1:{s:1:\"a\";i:1;}"));
    assert_eq!(c.maybe_unserialize(stored).unwrap(), array);
    assert_eq!(
        c.maybe_serialize(&s("a:0:{}")).unwrap(),
        s("s:6:\"a:0:{}\";")
    );
    assert_eq!(
        c.maybe_serialize(&HookValue::Int(5)).unwrap(),
        HookValue::Int(5)
    );
    assert_eq!(
        c.maybe_unserialize(s("b:0;")).unwrap(),
        HookValue::Bool(false)
    );
    assert!(matches!(
        c.maybe_unserialize(s("O:8:\"stdClass\":0:{}")).unwrap(),
        HookValue::Opaque(_)
    ));
    assert_eq!(c.maybe_unserialize(s("plain")).unwrap(), s("plain"));
}

#[test]
fn every_core_switch_name_is_refused() {
    // `show_on_front` is in WordPress 7.1.5's `sanitize_option()` switch, though the plan's copy left it out.
    let sanitizing = HookSanitizing::new(hooks());
    for n in ["show_on_front", "WPLANG", "admin_email"] {
        assert!(
            matches!(
                sanitizing.sanitize_option(&name(n), s("x")),
                Err(OptionsError::NeedsPhp { .. })
            ),
            "{n}"
        );
    }
}
