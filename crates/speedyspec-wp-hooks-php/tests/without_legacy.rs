//! A host that leaves out the legacy hook functions (`WpHooksArea::without_legacy`) keeps the native Hook API and
//! declares none of WordPress's names, so code that declares them itself loads.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`).

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::Value;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

const PROBE: &str = r#"return [
    function_exists('add_filter'),
    class_exists('WP_Hook', false),
    isset($GLOBALS['wp_filter']),
    function_exists('SpeedySpec\WpHooks\Native\add_filter'),
];"#;

fn expected() -> Value {
    Value::Array(
        [false, false, false, true]
            .into_iter()
            .enumerate()
            .map(|(i, b)| (speedyspec_php_ext::Key::Int(i as i64), Value::Bool(b)))
            .collect(),
    )
}

fn main() {
    let container = Rc::new(Container::new());
    let memory = Rc::new(MemoryHookStore::new());
    let hooks = WpHooksArea::new(container, memory).without_legacy();
    let areas: Vec<Rc<dyn NativeArea>> = vec![Rc::new(hooks)];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");

    let first = php.eval("probe", PROBE);
    check(
        "without_legacy_declares_only_the_native_api",
        matches!(&first, Ok(v) if *v == expected()),
        &first,
    );

    let declared = php.eval(
        "own",
        "function add_filter($a, $b) { return 'own'; } class WP_Hook {} return add_filter(1, 2);",
    );
    check(
        "code_may_declare_wordpress_names",
        matches!(&declared, Ok(Value::Str(s)) if s == "own"),
        &declared,
    );

    php.restart_request().expect("a second request starts");
    let second = php.eval("probe", PROBE);
    check(
        "a_second_request_still_has_no_legacy_names",
        matches!(&second, Ok(v) if *v == expected()),
        &second,
    );
}
