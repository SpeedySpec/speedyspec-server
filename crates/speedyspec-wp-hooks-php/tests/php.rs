//! WordPress's hook functions, WP_Hook and the hook globals from PHP, over the Rust Hook API.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`) and runs each case in order.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_php_ext::{Value, boundary_crossings, twins_area};
use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::{WpHooksArea, hooks_scenario_runner};

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

fn is(result: &Result<Value, PhpError>, expected: Value) -> bool {
    matches!(result, Ok(value) if *value == expected)
}

fn s(v: &str) -> HookValue {
    HookValue::Str(v.into())
}

fn suffix(label: &'static str) -> Callback {
    Callback::rust(
        &format!("rust-{label}"),
        move |args: &[HookValue]| match args.first() {
            Some(HookValue::Str(v)) => Ok(HookValue::Str(format!("{v}-{label}"))),
            _ => Ok(HookValue::Null),
        },
    )
}

fn main() {
    let container = Rc::new(Container::new());
    let memory = Rc::new(MemoryHookStore::new());
    let hooks = WpHooksArea::new(container.clone(), memory.clone());
    let api = hooks.api();
    // A worker callback, registered before any request.
    api.add_filter(
        &HookName::from("kept"),
        suffix("worker"),
        &HookValue::Int(10),
        1,
    )
    .unwrap();

    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("demo", container.clone())),
        Rc::new(hooks),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let mut runner = hooks_scenario_runner(php);

    // rust_caller_with_rust_callbacks_never_enters_php
    api.add_filter(&HookName::from("rust"), suffix("a"), &HookValue::Int(10), 1)
        .unwrap();
    api.add_filter(&HookName::from("rust"), suffix("b"), &HookValue::Int(5), 1)
        .unwrap();
    let before = boundary_crossings();
    let filtered = api
        .apply_filters(&HookName::from("rust"), s("x"), vec![])
        .unwrap();
    check(
        "rust_caller_with_rust_callbacks_never_enters_php",
        filtered == s("x-b-a") && boundary_crossings() == before,
        (filtered, boundary_crossings() - before),
    );

    // php_and_rust_callbacks_share_one_priority_order
    api.add_filter(
        &HookName::from("mixed"),
        suffix("rust"),
        &HookValue::Int(10),
        1,
    )
    .unwrap();
    let php = runner.php();
    let from_php = php.eval(
        "t",
        r#"add_filter('mixed', fn ($v) => "$v-php5", 5);
           add_filter('mixed', fn ($v) => "$v-php15", 15);
           return apply_filters('mixed', 'x');"#,
    );
    // A PHP callback's result passes through unconverted; a Rust caller reads it.
    let from_rust = api
        .apply_filters(&HookName::from("mixed"), s("y"), vec![])
        .unwrap()
        .read()
        .into_owned();
    check(
        "php_and_rust_callbacks_share_one_priority_order",
        is(&from_php, s("x-php5-rust-php15")) && from_rust == s("y-php5-rust-php15"),
        (&from_php, &from_rust),
    );

    // wp_filter_reads_and_iterates_like_wordpress
    let read = php.eval(
        "t",
        r#"global $wp_filter, $wp_actions, $wp_current_filter;
           $cb = fn ($v) => $v;
           add_filter('f', $cb, 7, 2);
           do_action('act');
           $hook = $wp_filter['f'];
           $names = [];
           foreach ($wp_filter as $name => $h) { $names[] = $name; }
           $wp_current_filter[] = 'pushed';
           $current = current_filter();
           unset($wp_current_filter[0]);
           return [
               isset($wp_filter['f']), !isset($wp_filter['nope']), $hook instanceof WP_Hook, $hook === $wp_filter['f'],
               $wp_filter['f'][7][array_key_first($wp_filter['f'][7])]['accepted_args'],
               $wp_filter['f'][7][array_key_first($wp_filter['f'][7])]['function'] === $cb,
               in_array('f', $names, true), count($wp_filter) === count($names),
               $wp_actions['act'], $current, current_filter(), $hook->has_filter('f', $cb),
           ];"#,
    );
    let expected = Value::Array(
        [
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Int(2),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Int(1),
            s("pushed"),
            Value::Bool(false),
            Value::Int(7),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, v)| (speedyspec_php_ext::Key::Int(i as i64), v))
        .collect(),
    );
    check(
        "wp_filter_reads_and_iterates_like_wordpress",
        is(&read, expected),
        &read,
    );

    // php_callbacks_cleared_at_request_end_rust_kept
    php.eval("t", r#"add_filter('kept', fn ($v) => "$v-php");"#)
        .unwrap();
    let during = php.eval("t", "return apply_filters('kept', 'x');");
    php.restart_request().unwrap();
    let after = php.eval("t", "return apply_filters('kept', 'x');");
    let mixed = api
        .apply_filters(&HookName::from("mixed"), s("y"), vec![])
        .unwrap();
    check(
        "php_callbacks_cleared_at_request_end_rust_kept",
        is(&during, s("x-worker-php")) && is(&after, s("x-worker")) && mixed == s("y"),
        (&during, &after, &mixed),
    );

    // empty_hooks_never_enter_php: no callbacks, no `all` hook: counted, value handed straight back
    let crossings_of = |code: &str| {
        let before = boundary_crossings();
        let result = php.eval("t", code);
        (result, boundary_crossings() - before)
    };
    // The loop alone, returning a fresh array as the measured code does (releasing it is one crossing).
    let (_, a_loop) = crossings_of(
        r#"$big = range(1, 100); for ($i = 0; $i < 100; $i++) { $x = $big; } return [$x === $big, count($x), 1];"#,
    );
    let (empty, empty_hooks) = crossings_of(
        r#"$big = range(1, 100);
           for ($i = 0; $i < 100; $i++) { $x = apply_filters('nothing', $big, 1); do_action('nothing_either', $big); }
           return [$x === $big, did_filter('nothing'), did_action('nothing_either')];"#,
    );
    check(
        "empty_hooks_never_enter_php",
        is(
            &empty,
            Value::Array(vec![
                (speedyspec_php_ext::Key::Int(0), Value::Bool(true)),
                (speedyspec_php_ext::Key::Int(1), Value::Int(100)),
                (speedyspec_php_ext::Key::Int(2), Value::Int(100)),
            ]),
        ) && empty_hooks == a_loop,
        (&empty, empty_hooks, a_loop),
    );

    // a_php_callback_is_one_crossing: plus one push and one pop of $wp_current_filter per run
    php.eval("t", r#"add_filter('one', fn ($v) => $v); add_action('one_action', fn ($a, $b) => null, 10, 2);"#)
        .unwrap();
    let (_, plain_loop) =
        crossings_of(r#"$big = range(1, 100); for ($i = 0; $i < 100; $i++) { $x = $big; }"#);
    let (_, filters) = crossings_of(
        r#"$big = range(1, 100); for ($i = 0; $i < 100; $i++) { $x = apply_filters('one', $big); }"#,
    );
    let (_, actions) = crossings_of(
        r#"$big = range(1, 100); for ($i = 0; $i < 100; $i++) { do_action('one_action', $big, 2); }"#,
    );
    check(
        "a_php_callback_is_one_crossing",
        filters - plain_loop == 300 && actions - plain_loop == 300,
        (filters - plain_loop, actions - plain_loop),
    );

    // named_callbacks_keep_their_reference: calling a function by name through the cached call takes nothing from
    // the name
    let refcounts = php.eval(
        "t",
        r#"function speedyspec_named_cb($v) { return "$v-named"; }
           $name = 'speedyspec_' . 'named_cb';
           $dump = function () use (&$name) { ob_start(); debug_zval_dump($name); return ob_get_clean(); };
           add_filter('named', $name);
           $before = $dump();
           $x = apply_filters('named', 'x');
           $y = apply_filters('named', 'y');
           $after = $dump();
           remove_filter('named', $name);
           return [$before === $after, $x, $y];"#,
    );
    check(
        "named_callbacks_keep_their_reference",
        is(
            &refcounts,
            Value::Array(vec![
                (speedyspec_php_ext::Key::Int(0), Value::Bool(true)),
                (speedyspec_php_ext::Key::Int(1), s("x-named")),
                (speedyspec_php_ext::Key::Int(2), s("y-named")),
            ]),
        ),
        &refcounts,
    );

    // replayed_scenarios_match_wordpress, with no worker callbacks (a fresh WordPress has none)
    api.remove_all_filters(&HookName::from("kept"), None)
        .unwrap();
    let mismatches = speedyspec_compat::replay("hooks", &mut runner);
    for mismatch in &mismatches {
        let text = mismatch.to_string();
        eprintln!("mismatch: {}", &text[..text.floor_char_boundary(700)]);
    }
    check(
        "replayed_scenarios_match_wordpress",
        mismatches.is_empty(),
        mismatches.len(),
    );
}
