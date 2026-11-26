//! `WpHookApi` as the `HookApi` port: every trait method, called through `dyn HookApi`, reaches the struct's own
//! method (a delegating impl that called itself would overflow the stack here).

use std::rc::Rc;

use speedyspec_wp_hooks::{IgnoreDeprecations, WpHookApi};
use speedyspec_wp_hooks_contracts::{
    ArrayKey, Callback, CallbackId, Count, HasFilterResult, HookApi, HookName, HookValue,
};
use speedyspec_wp_hooks_memory::MemoryHookStore;

fn name(hook: &str) -> HookName {
    HookName::from(hook)
}

fn append_a(args: &[HookValue]) -> Result<HookValue, speedyspec_wp_hooks_contracts::HookError> {
    match args.first() {
        Some(HookValue::Str(s)) => Ok(HookValue::Str(format!("{s}-a"))),
        other => Ok(other.cloned().unwrap_or(HookValue::Null)),
    }
}

#[test]
fn every_method_reaches_wp_hook_api_through_dyn() {
    let api = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let d: Rc<dyn HookApi> = api.clone();

    assert!(d.skip_if_empty(&name("empty"), Count::Filters).unwrap());
    assert_eq!(api.did_filter(&name("empty")).unwrap(), 1);

    assert!(
        d.add_filter(
            &name("t"),
            Callback::rust("append_a", append_a),
            &HookValue::Int(10),
            1
        )
        .unwrap()
    );
    let id = CallbackId(Some("append_a".into()));
    assert_eq!(
        d.has_filter(&name("t"), Some(&id), None).unwrap(),
        HasFilterResult::At(ArrayKey::Int(10))
    );
    assert_eq!(
        d.apply_filters(&name("t"), HookValue::Str("x".into()), vec![])
            .unwrap(),
        HookValue::Str("x-a".into())
    );
    assert!(
        d.remove_filter(&name("t"), id, &HookValue::Int(10))
            .unwrap()
    );
    assert_eq!(
        d.has_filter(&name("t"), None, None).unwrap(),
        HasFilterResult::No
    );

    d.do_action(&name("a"), vec![]).unwrap();
    d.do_action_ref_array(&name("a"), vec![]).unwrap();
    assert_eq!(api.did_action(&name("a")).unwrap(), 2);

    d.do_action_deprecated(
        &name("none"),
        vec![],
        &HookValue::Str("6.0.0".into()),
        &HookValue::Str("other".into()),
        &HookValue::Str(String::new()),
    )
    .unwrap();
    assert_eq!(api.did_action(&name("none")).unwrap(), 0);
}
