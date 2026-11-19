//! A PHP hook store bound in `app.<id>.php.bindings` takes every registration, from PHP and from Rust (Review
//! Focus 4). Its own process: the binding applies to every request.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::{Key, Value, twins_area};
use speedyspec_wp_hooks::{Callback, HookName, HookStore, HookValue};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::{HOOK_STORE, PhpHookStore, WpHooksArea};

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

fn main() {
    let container = Rc::new(Container::new());
    let memory = Rc::new(MemoryHookStore::new());
    let hooks = WpHooksArea::new(container.clone(), memory.clone());
    let api = hooks.api();
    let store = ContainerArea::new("demo", container.clone())
        .port::<dyn HookStore>(HOOK_STORE, |object| Rc::new(PhpHookStore::new(object)))
        .stateful(HOOK_STORE)
        .bindings(vec![(HOOK_STORE.to_string(), "ArrayHookStore".to_string())]);
    let areas: Vec<Rc<dyn NativeArea>> = vec![twins_area(), Rc::new(store), Rc::new(hooks)];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");

    php.eval(
        "t",
        r#"final class ArrayHookStore implements \SpeedySpec\WpHooks\HookStorage
        {
            public static array $log = [];
            private array $hooks = [];
            private array $counts = ['actions' => [], 'filters' => []];
            private array $stack = [];
            public function hook($name) { return $this->hooks[$name] ?? null; }
            public function hooks() { return $this->hooks; }
            public function insert($name, $hook): void { self::$log[] = "insert $name"; $this->hooks[$name] = $hook; }
            public function remove($name): void { self::$log[] = "remove $name"; unset($this->hooks[$name]); }
            public function clear(): void { $this->hooks = []; }
            public function counts(string $kind) { return $this->counts[$kind]; }
            public function set_count(string $kind, $name, $value): void {
                if ($value === null) { unset($this->counts[$kind][$name]); } else { $this->counts[$kind][$name] = $value; }
            }
            public function current_stack() { return $this->stack; }
            public function set_current_stack($stack): void { $this->stack = $stack; }
        }"#,
    )
    .expect("the store class is declared");

    let from_php = php.eval(
        "t",
        r#"add_filter('p', fn ($v) => "$v-php"); return apply_filters('p', 'x');"#,
    );
    let rust = Callback::rust("rust", |args: &[HookValue]| match args.first() {
        Some(HookValue::Str(v)) => Ok(HookValue::Str(format!("{v}-rust"))),
        _ => Ok(HookValue::Null),
    });
    api.add_filter(&HookName::from("r"), rust, &HookValue::Int(10), 1)
        .unwrap();
    let from_rust = api.apply_filters(&HookName::from("r"), HookValue::Str("y".into()), vec![]);
    let log = php.eval(
        "t",
        "return [ArrayHookStore::$log, did_filter('p'), did_filter('r')];",
    );
    let expected = Value::Array(vec![
        (
            Key::Int(0),
            Value::Array(vec![
                (Key::Int(0), Value::Str("insert p".into())),
                (Key::Int(1), Value::Str("insert r".into())),
            ]),
        ),
        (Key::Int(1), Value::Int(1)),
        (Key::Int(2), Value::Int(1)),
    ]);
    check(
        "php_hook_store_binding_takes_every_registration",
        matches!(&from_php, Ok(Value::Str(v)) if v == "x-php")
            && matches!(&from_rust, Ok(HookValue::Str(v)) if v == "y-rust")
            && matches!(&log, Ok(v) if *v == expected)
            && memory.hooks().is_empty(),
        (&from_php, &from_rust, &log),
    );
}
