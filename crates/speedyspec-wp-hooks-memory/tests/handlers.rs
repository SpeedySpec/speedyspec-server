//! WordPress's Hook API through the buses, over the memory store, with Rust callbacks.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{CommandBus, QueryBus};
use speedyspec_wp_hooks::{
    AddAction, AddFilter, ApplyFilters, ApplyFiltersDeprecated, ApplyFiltersRefArray,
    CallbackIdentity, CallbackShape, CurrentAction, CurrentFilter, CurrentPriority,
    DeprecatedHooks, DidAction, DidFilter, DoAction, DoActionDeprecated, DoActionRefArray,
    DoingAction, DoingFilter, HasAction, HasFilter, HookStore, RemoveAction, RemoveAllActions,
    RemoveAllFilters, RemoveFilter, WpHookApi, register_hook_handlers,
};
use speedyspec_wp_hooks_contracts::{
    ArrayKey, Callback, CallbackId, Count, HasFilterResult, HookError, HookKey, HookName, HookValue,
};
use speedyspec_wp_hooks_memory::MemoryHookStore;

type Log = Rc<RefCell<Vec<String>>>;

struct World {
    commands: CommandBus,
    queries: QueryBus,
    store: Rc<MemoryHookStore>,
    api: Rc<WpHookApi>,
    log: Log,
    deprecated: Log,
}

struct Notes(Log);

impl DeprecatedHooks for Notes {
    fn deprecated_hook(
        &self,
        hook: &HookValue,
        version: &HookValue,
        _replacement: &HookValue,
        _message: &HookValue,
    ) -> Result<(), HookError> {
        self.0.borrow_mut().push(format!("{hook:?} {version:?}"));
        Ok(())
    }
}

fn world() -> World {
    let container = Rc::new(Container::new());
    let store = Rc::new(MemoryHookStore::new());
    let deprecated: Log = Rc::default();
    let api = Rc::new(WpHookApi::new(
        store.clone(),
        Rc::new(Notes(deprecated.clone())),
    ));
    register_hook_handlers(&container, api.clone());
    World {
        commands: CommandBus::new(container.clone()),
        queries: QueryBus::new(container),
        store,
        api,
        log: Rc::default(),
        deprecated,
    }
}

fn s(v: &str) -> HookValue {
    HookValue::Str(v.to_string())
}

fn int(n: i64) -> HookValue {
    HookValue::Int(n)
}

/// A callback that logs `label` and appends `-label` to its first argument.
fn append(w: &World, label: &str) -> Callback {
    let (log, label) = (w.log.clone(), label.to_string());
    Callback::rust(&format!("append-{label}"), move |args: &[HookValue]| {
        log.borrow_mut().push(label.clone());
        match args.first() {
            Some(HookValue::Str(v)) => Ok(HookValue::Str(format!("{v}-{label}"))),
            _ => Ok(HookValue::Null),
        }
    })
}

fn id(callback: &Callback) -> CallbackId {
    CallbackId(callback.id().map(str::to_string))
}

impl World {
    fn add(&self, hook: &str, callback: &Callback, priority: i64) {
        let added = self.commands.dispatch(AddFilter {
            hook: s(hook).into(),
            callback: callback.clone(),
            priority: int(priority),
            accepted_args: 1,
        });
        assert!(added.unwrap().unwrap());
    }

    fn apply(&self, hook: &str, value: &str) -> HookValue {
        self.commands
            .dispatch(ApplyFilters {
                hook: s(hook).into(),
                value: s(value),
                args: vec![],
            })
            .unwrap()
            .unwrap()
    }

    fn api(&self) -> Rc<WpHookApi> {
        self.api.clone()
    }
}

// ----- each command and query

#[test]
fn add_and_apply_filters_in_priority_order() {
    let w = world();
    w.add("f", &append(&w, "a"), 20);
    w.add("f", &append(&w, "b"), 5);
    w.add("f", &append(&w, "c"), 10);
    w.add("f", &append(&w, "d"), 10);
    assert_eq!(w.apply("f", "x"), s("x-b-c-d-a"));
    assert_eq!(w.apply("none", "v"), s("v"));
    assert_eq!(
        w.queries
            .ask(DidFilter {
                hook: s("f").into()
            })
            .unwrap()
            .unwrap(),
        1
    );
    assert_eq!(
        w.queries
            .ask(DidFilter {
                hook: s("none").into()
            })
            .unwrap()
            .unwrap(),
        1
    );
    assert_eq!(
        w.queries
            .ask(DidFilter {
                hook: s("never").into()
            })
            .unwrap()
            .unwrap(),
        0
    );
}

#[test]
fn string_numeric_priorities_sort_numerically() {
    let w = world();
    let (a, b, c) = (append(&w, "a"), append(&w, "b"), append(&w, "c"));
    for (cb, p) in [(&a, s("5")), (&b, int(10)), (&c, s(" 7"))] {
        w.commands
            .dispatch(AddFilter {
                hook: s("f").into(),
                callback: cb.clone(),
                priority: p,
                accepted_args: 1,
            })
            .unwrap()
            .unwrap();
    }
    assert_eq!(
        w.apply("f", "x"),
        s("x-a-c-b"),
        "recorded from WordPress"
    );
    let has = w
        .queries
        .ask(HasFilter {
            hook: s("f").into(),
            callback: Some(id(&a)),
            priority: None,
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        has,
        HasFilterResult::At(ArrayKey::Int(5)),
        "\"5\" is the integer key 5"
    );
    let has = w
        .queries
        .ask(HasFilter {
            hook: s("f").into(),
            callback: Some(id(&c)),
            priority: None,
        })
        .unwrap()
        .unwrap();
    assert_eq!(has, HasFilterResult::At(ArrayKey::Str(" 7".into())));
}

#[test]
fn extreme_priorities() {
    let w = world();
    w.add("f", &append(&w, "a"), i64::MAX);
    w.add("f", &append(&w, "b"), i64::MIN);
    w.add("f", &append(&w, "c"), 0);
    assert_eq!(w.apply("f", "x"), s("x-b-c-a"));
}

#[test]
fn add_action_and_do_action() {
    let w = world();
    let seen: Log = Rc::default();
    let sink = seen.clone();
    let counter = Callback::rust("count", move |args: &[HookValue]| {
        sink.borrow_mut().push(format!("{args:?}"));
        Ok(HookValue::Int(args.len() as i64))
    });
    w.commands
        .dispatch(AddAction {
            hook: s("a").into(),
            callback: counter.clone(),
            priority: int(10),
            accepted_args: 2,
        })
        .unwrap()
        .unwrap();
    w.commands
        .dispatch(DoAction {
            hook: s("a").into(),
            args: vec![int(1), int(2)],
        })
        .unwrap()
        .unwrap();
    w.commands
        .dispatch(DoAction {
            hook: s("a").into(),
            args: vec![],
        })
        .unwrap()
        .unwrap();
    w.commands
        .dispatch(DoActionRefArray {
            hook: s("a").into(),
            args: vec![int(3)],
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        *seen.borrow(),
        vec!["[Int(1), Int(2)]", "[Str(\"\")]", "[Int(3)]"]
    );
    assert_eq!(
        w.queries
            .ask(DidAction {
                hook: s("a").into()
            })
            .unwrap()
            .unwrap(),
        3
    );
    assert_eq!(
        w.queries
            .ask(DidAction {
                hook: s("never").into()
            })
            .unwrap()
            .unwrap(),
        0
    );
}

#[test]
fn has_filter_variants() {
    let w = world();
    let (a, b) = (append(&w, "a"), append(&w, "b"));
    let ask = |callback: Option<CallbackId>, priority: Option<i64>| {
        w.queries
            .ask(HasFilter {
                hook: s("f").into(),
                callback,
                priority,
            })
            .unwrap()
            .unwrap()
    };
    assert_eq!(ask(None, None), HasFilterResult::No);
    w.add("f", &a, 7);
    assert_eq!(ask(None, None), HasFilterResult::Any);
    assert_eq!(
        ask(Some(id(&a)), None),
        HasFilterResult::At(ArrayKey::Int(7))
    );
    assert_eq!(ask(Some(id(&b)), None), HasFilterResult::No);
    assert_eq!(ask(Some(id(&a)), Some(7)), HasFilterResult::Any);
    assert_eq!(ask(Some(id(&a)), Some(8)), HasFilterResult::No);
    assert_eq!(
        ask(Some(CallbackId(None)), None),
        HasFilterResult::No,
        "an unidentifiable callback"
    );
    let action = w
        .queries
        .ask(HasAction {
            hook: s("f").into(),
            callback: Some(id(&a)),
            priority: None,
        })
        .unwrap()
        .unwrap();
    assert_eq!(action, HasFilterResult::At(ArrayKey::Int(7)));
}

#[test]
fn remove_filter_and_remove_all() {
    let w = world();
    let (a, b, c) = (append(&w, "a"), append(&w, "b"), append(&w, "c"));
    w.add("f", &a, 10);
    w.add("f", &b, 10);
    let remove = |cb: &Callback, p: i64| {
        w.commands
            .dispatch(RemoveFilter {
                hook: s("f").into(),
                callback: id(cb),
                priority: int(p),
            })
            .unwrap()
            .unwrap()
    };
    assert!(!remove(&a, 11));
    assert!(remove(&a, 10));
    assert!(!remove(&a, 10));
    assert_eq!(w.apply("f", "x"), s("x-b"));
    assert!(remove(&b, 10));
    assert!(
        !w.store.has_hook(&HookKey::new(ArrayKey::Str("f".into()))),
        "an emptied hook leaves $wp_filter"
    );
    w.add("f", &a, 1);
    w.add("f", &b, 2);
    w.add("f", &c, 2);
    let all = w
        .commands
        .dispatch(RemoveAllFilters {
            hook: s("f").into(),
            priority: Some(int(2)),
        })
        .unwrap()
        .unwrap();
    assert!(all);
    assert_eq!(w.apply("f", "x"), s("x-a"));
    assert!(
        w.commands
            .dispatch(RemoveAllFilters {
                hook: s("f").into(),
                priority: None
            })
            .unwrap()
            .unwrap()
    );
    assert!(!w.store.has_hook(&HookKey::new(ArrayKey::Str("f".into()))));
    w.add("a", &a, 3);
    assert!(
        w.commands
            .dispatch(RemoveAction {
                hook: s("a").into(),
                callback: id(&a),
                priority: int(3)
            })
            .unwrap()
            .unwrap()
    );
    w.add("a", &a, 3);
    assert!(
        w.commands
            .dispatch(RemoveAllActions {
                hook: s("a").into(),
                priority: None
            })
            .unwrap()
            .unwrap()
    );
    assert!(!w.store.has_hook(&HookKey::new(ArrayKey::Str("a".into()))));
}

#[test]
fn same_callback_twice_at_one_priority_is_one_entry() {
    let w = world();
    let a = append(&w, "a");
    w.add("f", &a, 10);
    w.add("f", &a, 10);
    assert_eq!(w.apply("f", "x"), s("x-a"));
}

#[test]
fn ref_array_variants() {
    let w = world();
    w.add("f", &append(&w, "a"), 10);
    let filtered = w
        .commands
        .dispatch(ApplyFiltersRefArray {
            hook: s("f").into(),
            args: vec![s("x"), s("y")],
        })
        .unwrap();
    assert_eq!(filtered.unwrap(), s("x-a"));
}

#[test]
fn deprecated_variants_notify_only_with_callbacks() {
    let w = world();
    let deprecated = |hook: &str| ApplyFiltersDeprecated {
        hook: s(hook).into(),
        args: vec![s("v")],
        version: s("1.0"),
        replacement: s("new"),
        message: s(""),
    };
    assert_eq!(
        w.commands.dispatch(deprecated("old")).unwrap().unwrap(),
        s("v")
    );
    assert!(w.deprecated.borrow().is_empty());
    w.add("old", &append(&w, "a"), 10);
    assert_eq!(
        w.commands.dispatch(deprecated("old")).unwrap().unwrap(),
        s("v-a")
    );
    assert_eq!(w.deprecated.borrow().len(), 1);
    let action = DoActionDeprecated {
        hook: s("olda").into(),
        args: vec![int(1)],
        version: s("1.0"),
        replacement: s(""),
        message: s(""),
    };
    w.commands.dispatch(action).unwrap().unwrap();
    assert_eq!(w.deprecated.borrow().len(), 1, "no callbacks, no notice");
    assert_eq!(
        w.queries
            .ask(DidAction {
                hook: s("olda").into()
            })
            .unwrap()
            .unwrap(),
        0
    );
}

#[test]
fn current_and_doing() {
    let w = world();
    assert_eq!(w.queries.ask(CurrentFilter).unwrap(), None);
    assert_eq!(w.queries.ask(CurrentAction).unwrap(), None);
    assert!(!w.queries.ask(DoingFilter { hook: None }).unwrap());
    assert!(!w.queries.ask(DoingAction { hook: Some(s("a")) }).unwrap());
    let api = w.api();
    let seen: Log = Rc::default();
    let sink = seen.clone();
    let probe = Callback::rust("probe", move |args: &[HookValue]| {
        sink.borrow_mut().push(format!(
            "{:?} {} {} {:?}",
            api.current_filter(),
            api.doing_filter(None),
            api.doing_filter(Some(&HookValue::Str("a".into()))),
            api.current_priority(&HookName::from("a")).unwrap()
        ));
        Ok(args.first().cloned().unwrap_or(HookValue::Null))
    });
    w.add("a", &probe, 4);
    w.commands
        .dispatch(DoAction {
            hook: s("a").into(),
            args: vec![],
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        *seen.borrow(),
        vec!["Some(Str(\"a\")) true true Some(Int(4))"]
    );
    let outside = w
        .queries
        .ask(CurrentPriority {
            hook: s("a").into(),
        })
        .unwrap()
        .unwrap();
    assert_eq!(outside, None);
}

#[test]
fn exception_leaves_the_current_filter_as_wordpress_does() {
    let w = world();
    w.add(
        "f",
        &Callback::rust("boom", |_: &[HookValue]| Err(HookError::message("boom"))),
        10,
    );
    let err = w
        .commands
        .dispatch(ApplyFilters {
            hook: s("f").into(),
            value: s("x"),
            args: vec![],
        })
        .unwrap();
    assert_eq!(err.unwrap_err().to_string(), "boom");
    assert_eq!(
        w.queries.ask(CurrentFilter).unwrap(),
        Some(s("f")),
        "recorded: WordPress does not pop on a throw"
    );
}

// ----- HookIteration (Review Focus 2), as recorded from WordPress

/// A callback that adds `added` to `f` at `priority` the first time it runs.
fn adder(w: &World, priority: i64, added: Callback) -> Callback {
    let api = w.api();
    let done = Rc::new(RefCell::new(false));
    Callback::rust(&format!("adder-{priority}"), move |args: &[HookValue]| {
        if !done.replace(true) {
            api.add_filter(
                &HookName::from("f"),
                added.clone(),
                &HookValue::Int(priority),
                1,
            )
            .unwrap();
        }
        Ok(args.first().cloned().unwrap_or(HookValue::Null))
    })
}

fn remover(w: &World, priority: i64, removed: &Callback, label: &str) -> Callback {
    let (api, removed) = (w.api(), id(removed));
    Callback::rust(label, move |args: &[HookValue]| {
        api.remove_filter(
            &HookName::from("f"),
            removed.clone(),
            &HookValue::Int(priority),
        )
        .unwrap();
        Ok(args.first().cloned().unwrap_or(HookValue::Null))
    })
}

#[test]
fn add_at_current_priority_runs() {
    // WordPress iterates a priority's callbacks over a snapshot: the new callback runs from the next run on.
    let w = world();
    w.add("f", &adder(&w, 10, append(&w, "c")), 10);
    w.add("f", &append(&w, "a"), 10);
    assert_eq!(w.apply("f", "x"), s("x-a"));
    assert_eq!(w.apply("f", "y"), s("y-a-c"));
}

#[test]
fn add_at_earlier_priority_waits() {
    let w = world();
    w.add("f", &adder(&w, 5, append(&w, "c")), 10);
    w.add("f", &append(&w, "a"), 20);
    assert_eq!(w.apply("f", "x"), s("x-a"));
    assert_eq!(w.apply("f", "y"), s("y-c-a"));
}

#[test]
fn add_at_later_priority_runs() {
    let w = world();
    w.add("f", &adder(&w, 15, append(&w, "c")), 10);
    w.add("f", &append(&w, "a"), 20);
    assert_eq!(w.apply("f", "x"), s("x-c-a"));
}

#[test]
fn add_at_new_lowest_priority_waits() {
    let w = world();
    w.add("f", &append(&w, "a"), 5);
    w.add("f", &adder(&w, 1, append(&w, "c")), 10);
    w.add("f", &append(&w, "b"), 20);
    assert_eq!(w.apply("f", "x"), s("x-a-b"));
    assert_eq!(w.apply("f", "y"), s("y-c-a-b"));
}

#[test]
fn remove_later_priority_skips_it() {
    let w = world();
    let a = append(&w, "a");
    w.add("f", &remover(&w, 20, &a, "remover"), 10);
    w.add("f", &a, 20);
    w.add("f", &append(&w, "b"), 30);
    assert_eq!(w.apply("f", "x"), s("x-b"));
}

#[test]
fn remove_at_current_priority_still_runs_this_time() {
    let w = world();
    let a = append(&w, "a");
    w.add("f", &remover(&w, 10, &a, "remover"), 10);
    w.add("f", &a, 10);
    assert_eq!(w.apply("f", "x"), s("x-a"));
    assert_eq!(w.apply("f", "y"), s("y"));
}

#[test]
fn remove_self_mid_run() {
    let w = world();
    let api = w.api();
    let me = Rc::new(RefCell::new(None::<CallbackId>));
    let mine = me.clone();
    let once = Callback::rust("once", move |args: &[HookValue]| {
        let id = mine.borrow().clone().unwrap();
        api.remove_filter(&HookName::from("f"), id, &HookValue::Int(10))
            .unwrap();
        Ok(HookValue::Str(format!("{:?}-once", args[0])))
    });
    *me.borrow_mut() = Some(id(&once));
    w.add("f", &once, 10);
    w.add("f", &append(&w, "a"), 20);
    assert_eq!(w.apply("f", "x"), s("Str(\"x\")-once-a"));
    assert_eq!(w.apply("f", "y"), s("y-a"));
}

#[test]
fn recursive_do_action_keeps_outer_position() {
    let w = world();
    let api = w.api();
    let depth = Rc::new(RefCell::new(0));
    let log = w.log.clone();
    let again = Callback::rust("again", move |_: &[HookValue]| {
        log.borrow_mut().push("again".into());
        if *depth.borrow() < 1 {
            *depth.borrow_mut() += 1;
            api.do_action(&HookName::from("a"), vec![]).unwrap();
        }
        Ok(HookValue::Null)
    });
    w.add("a", &append(&w, "first"), 5);
    w.add("a", &again, 10);
    w.add("a", &append(&w, "last"), 15);
    w.commands
        .dispatch(DoAction {
            hook: s("a").into(),
            args: vec![],
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        *w.log.borrow(),
        vec!["first", "again", "first", "again", "last", "last"]
    );
    assert_eq!(
        w.queries
            .ask(DidAction {
                hook: s("a").into()
            })
            .unwrap()
            .unwrap(),
        2
    );
}

#[test]
fn recursive_filter_matches_the_fixture() {
    let w = world();
    let api = w.api();
    let depth = Rc::new(RefCell::new(0));
    let again = Callback::rust("again", move |args: &[HookValue]| {
        let value = args[0].clone();
        if *depth.borrow() < 2 {
            *depth.borrow_mut() += 1;
            let out = api
                .apply_filters(&HookName::from("f"), value, vec![])
                .unwrap();
            *depth.borrow_mut() -= 1;
            return Ok(out);
        }
        Ok(value)
    });
    w.add("f", &append(&w, "a"), 5);
    w.add("f", &again, 10);
    w.add("f", &append(&w, "b"), 15);
    assert_eq!(
        w.apply("f", "x"),
        s("x-a-a-a-b-b-b"),
        "recorded as recursive_filter"
    );
}

// ----- the all hook, accepted_args

#[test]
fn all_hook_sees_every_hook() {
    let w = world();
    let seen: Log = Rc::default();
    let sink = seen.clone();
    let all = Callback::rust("all", move |args: &[HookValue]| {
        sink.borrow_mut().push(format!("{args:?}"));
        Ok(HookValue::Null)
    });
    w.commands
        .dispatch(AddAction {
            hook: s("all").into(),
            callback: all,
            priority: int(10),
            accepted_args: 1,
        })
        .unwrap()
        .unwrap();
    w.add("a", &append(&w, "a"), 10);
    w.commands
        .dispatch(DoAction {
            hook: s("a").into(),
            args: vec![int(1), int(2)],
        })
        .unwrap()
        .unwrap();
    w.commands
        .dispatch(ApplyFilters {
            hook: s("f").into(),
            value: s("v"),
            args: vec![int(3)],
        })
        .unwrap()
        .unwrap();
    w.commands
        .dispatch(DoActionRefArray {
            hook: s("r").into(),
            args: vec![int(4)],
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        *seen.borrow(),
        vec![
            "[Str(\"a\"), Int(1), Int(2)]",
            "[Str(\"f\"), Str(\"v\"), Int(3)]",
            "[Str(\"r\"), Array([(Int(0), Int(4))])]",
        ],
        "all ignores accepted_args and gets func_get_args()"
    );
    assert_eq!(
        w.queries.ask(CurrentFilter).unwrap(),
        None,
        "the stack is popped for hooks with no callbacks"
    );
}

#[test]
fn accepted_args_truncates() {
    let w = world();
    let counts: Log = Rc::default();
    for accepted in [0, 1, 2, 5, -1] {
        let sink = counts.clone();
        let cb = Callback::rust(&format!("count{accepted}"), move |args: &[HookValue]| {
            sink.borrow_mut().push(format!("{accepted}:{}", args.len()));
            Ok(HookValue::Str("v".into()))
        });
        w.commands
            .dispatch(AddFilter {
                hook: s("f").into(),
                callback: cb,
                priority: int(10),
                accepted_args: accepted,
            })
            .unwrap()
            .unwrap();
    }
    w.commands
        .dispatch(ApplyFilters {
            hook: s("f").into(),
            value: s("v"),
            args: vec![int(1), int(2)],
        })
        .unwrap()
        .unwrap();
    assert_eq!(*counts.borrow(), vec!["0:0", "1:1", "2:2", "5:3", "-1:2"]);
}

// ----- identity

#[test]
fn callback_identity_matches_wordpress() {
    // The fixture of the hooks area's release: its source.json override, else compat/wordpress.json's pin.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../compat/hooks");
    let json = |path: String| -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    };
    let source = json(format!("{dir}/source.json"));
    let table = json(format!("{dir}/../wordpress.json"));
    let version = source["wordpress"]
        .as_str()
        .or(table["wordpress"].as_str())
        .unwrap();
    let fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{dir}/wordpress-{version}.json")).unwrap(),
    )
    .unwrap();
    let recorded = |scenario: &str| {
        fixture["results"][scenario][3]["returned"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let shapes = [
        (
            "callback_kind_function",
            CallbackShape::Name("speedyspec_fixture_k_function".into()),
        ),
        (
            "callback_kind_static",
            CallbackShape::Static {
                class: "SpeedySpecFixture_k_static".into(),
                method: "invoke".into(),
            },
        ),
        (
            "callback_kind_static_string",
            CallbackShape::Name("SpeedySpecFixture_k_static_string::invoke".into()),
        ),
        (
            "callback_kind_method",
            CallbackShape::Method {
                object: 41,
                method: "invoke".into(),
            },
        ),
        ("callback_kind_invokable", CallbackShape::Object(42)),
        ("callback_kind_closure", CallbackShape::Object(43)),
    ];
    for (scenario, shape) in shapes {
        let id = CallbackIdentity::unique_id(&shape).unwrap();
        let normalised = id
            .replace("41", "{k_method}")
            .replace("42", "{k_invokable}")
            .replace("43", "{k_closure}");
        assert_eq!(normalised, recorded(scenario), "{scenario}");
    }
    assert_eq!(
        CallbackIdentity::unique_id(&CallbackShape::Name("strtoupper".into())).unwrap(),
        recorded("builtin_function_as_callback")
    );
    assert_eq!(CallbackIdentity::unique_id(&CallbackShape::Invalid), None);
}

// ----- partitions

#[test]
fn request_partition_cleared_keeps_worker_callbacks() {
    let w = world();
    w.add("f", &append(&w, "worker"), 10);
    w.store.start_request();
    w.add("f", &append(&w, "request"), 5);
    w.add("g", &append(&w, "request"), 10);
    assert_eq!(w.apply("f", "x"), s("x-request-worker"));
    w.store.end_request();
    assert_eq!(w.apply("f", "x"), s("x-worker"));
    assert!(!w.store.has_hook(&HookKey::new(ArrayKey::Str("g".into()))));
    assert_eq!(
        w.queries
            .ask(DidFilter {
                hook: s("f").into()
            })
            .unwrap()
            .unwrap(),
        1,
        "counts are per request"
    );
}

// ----- kept sorted when added, walked when fired

#[test]
fn priorities_are_sorted_when_added_as_ksort_would() {
    let w = world();
    // Integers, numeric strings and non-numeric strings, added out of order.
    for (label, priority) in [
        ("p20", int(20)),
        ("pm5", int(-5)),
        ("s7", s(" 7")),
        ("p10", int(10)),
        ("abc", s("abc")),
        ("p0", int(0)),
        ("s15x", s("15x")),
    ] {
        w.api()
            .add_filter(&HookName::from("f"), append(&w, label), &priority, 1)
            .unwrap();
    }
    let hook = w
        .store
        .hook(&HookKey::new(ArrayKey::Str("f".into())))
        .unwrap();
    let sorted: Vec<ArrayKey> = hook.state().callbacks.keys().cloned().collect();
    // The order a full stable `ksort(SORT_NUMERIC)` gives the same keys in insertion order.
    let mut by_ksort = vec![
        ArrayKey::Int(20),
        ArrayKey::Int(-5),
        ArrayKey::Str(" 7".into()),
        ArrayKey::Int(10),
        ArrayKey::Str("abc".into()),
        ArrayKey::Int(0),
        ArrayKey::Str("15x".into()),
    ];
    by_ksort.sort_by(speedyspec_wp_hooks::PriorityOrder::compare);
    assert_eq!(sorted, by_ksort);
    assert_eq!(w.apply("f", "x"), s("x-pm5-abc-p0-s7-p10-s15x-p20"));
}

#[test]
fn offset_set_order_is_kept_until_a_new_priority_sorts_it() {
    let w = world();
    w.add("f", &append(&w, "a"), 10);
    let hook = w
        .store
        .hook(&HookKey::new(ArrayKey::Str("f".into())))
        .unwrap();
    // `$hook[1] = [...]` appends priority 1 after 10, as a PHP array assignment does.
    let slice = hook.state().callbacks[&ArrayKey::Int(10)].as_ref().clone();
    hook.set_priority(Some(ArrayKey::Int(1)), slice);
    let keys = |hook: &speedyspec_wp_hooks::WpHook| -> Vec<ArrayKey> {
        hook.state().callbacks.keys().cloned().collect()
    };
    assert_eq!(keys(&hook), [ArrayKey::Int(10), ArrayKey::Int(1)]);
    // A callback at an existing priority sorts nothing.
    w.add("f", &append(&w, "b"), 10);
    assert_eq!(keys(&hook), [ArrayKey::Int(10), ArrayKey::Int(1)]);
    // A new priority sorts the whole list, as WordPress's ksort() does.
    w.add("f", &append(&w, "c"), 5);
    assert_eq!(
        keys(&hook),
        [ArrayKey::Int(1), ArrayKey::Int(5), ArrayKey::Int(10)]
    );
}

#[test]
fn empty_hooks_are_counted_and_skipped() {
    let w = world();
    let api = w.api();
    let none = HookName::from("none");
    assert!(api.skip_if_empty(&none, Count::Filters).unwrap());
    assert!(api.skip_if_empty(&none, Count::Actions).unwrap());
    assert_eq!(
        api.did_filter(&none).unwrap(),
        1,
        "the skipped run is counted"
    );
    assert_eq!(api.did_action(&none).unwrap(), 1);
    w.add("f", &append(&w, "a"), 10);
    assert!(
        !api.skip_if_empty(&HookName::from("f"), Count::Filters)
            .unwrap()
    );
    assert_eq!(
        api.did_filter(&HookName::from("f")).unwrap(),
        0,
        "not skipped: not counted here"
    );
    // An `all` hook runs for every hook, so nothing is skipped.
    w.add("all", &append(&w, "all"), 10);
    assert!(!api.skip_if_empty(&none, Count::Filters).unwrap());
    assert!(
        api.skip_if_empty(&HookName::new(HookValue::Array(vec![])), Count::Filters)
            .is_err(),
        "an array is not a hook name"
    );
}
