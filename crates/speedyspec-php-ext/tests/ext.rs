//! The speedyspec_wordpress module, PHP twins of ports, the boundary counter and the request cycle.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`) and runs each case in order.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::{PhpObject, boundary_crossings, php_port, twins_area};

#[php_port(name = "SpeedySpec\\Testing\\Greeter")]
pub trait Greeter {
    fn greet(&self, name: String) -> String;
    fn count(&self, n: i64) -> i64;
}

/// A Rust adapter of the same port.
struct RustGreeter;

impl Greeter for RustGreeter {
    fn greet(&self, name: String) -> String {
        format!("hello {name}")
    }
    fn count(&self, n: i64) -> i64 {
        n + 1
    }
}

/// Records request starts and ends, to show the end runs even after a fatal error.
#[derive(Default)]
struct Recorder(RefCell<Vec<&'static str>>);

impl NativeArea for Recorder {
    fn request_start(&self) {
        self.0.borrow_mut().push("start");
    }
    fn request_end(&self) {
        self.0.borrow_mut().push("end");
    }
}

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

fn main() {
    let recorder = Rc::new(Recorder::default());
    let areas: Vec<Rc<dyn NativeArea>> = vec![twins_area(), recorder.clone()];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");

    // module_name_is_speedyspec_wordpress
    let loaded = php
        .eval("t", "return extension_loaded('speedyspec_wordpress');")
        .unwrap();
    check(
        "module_name_is_speedyspec_wordpress",
        loaded == Value::Bool(true),
        loaded,
    );

    // php_port_twin_round_trips
    let object = php
        .eval(
            "t",
            r#"return new class implements \SpeedySpec\Testing\Greeter {
                public function greet(string $name): string { return "hi $name"; }
                public function count(int $n): int { return $n * 2; }
            };"#,
        )
        .expect("a PHP class implements the twin");
    let port = PhpGreeter::new(PhpObject::new(object).expect("an object"));
    let greeting = port.greet("ada".to_string());
    let counted = port.count(21);
    check(
        "php_port_twin_round_trips",
        greeting == "hi ada" && counted == 42,
        (greeting, counted),
    );

    // rust_call_never_crosses_boundary
    let rust: Box<dyn Greeter> = Box::new(RustGreeter);
    let before = boundary_crossings();
    let _ = rust.greet("ada".to_string());
    let _ = rust.count(1);
    check(
        "rust_call_never_crosses_boundary",
        boundary_crossings() == before,
        boundary_crossings() - before,
    );

    // php_call_crosses_boundary (the counter does count)
    let before = boundary_crossings();
    let _ = port.count(1);
    check(
        "php_call_crosses_boundary",
        boundary_crossings() > before,
        boundary_crossings() - before,
    );

    // request_end_runs_after_fatal_error (Review Focus 1)
    drop(port);
    recorder.0.borrow_mut().clear();
    let fatal = php.eval("t", "undefined_function_here();");
    check("fatal_error_reported", fatal.is_err(), &fatal);
    php.restart_request().expect("the next request starts");
    let events = recorder.0.borrow().clone();
    check(
        "request_end_runs_after_fatal_error",
        events == vec!["end", "start"],
        &events,
    );
    let twin = php
        .eval(
            "t",
            r"return interface_exists('SpeedySpec\Testing\Greeter');",
        )
        .unwrap();
    check(
        "twin_declared_again_next_request",
        twin == Value::Bool(true),
        twin,
    );
    let clean = php
        .eval("t", "return isset($GLOBALS['leftover']);")
        .unwrap();
    php.eval("t", "$GLOBALS['leftover'] = 1;").unwrap();
    php.restart_request().unwrap();
    let after = php
        .eval("t", "return isset($GLOBALS['leftover']);")
        .unwrap();
    check(
        "globals_cleared_between_requests",
        clean == Value::Bool(false) && after == Value::Bool(false),
        after,
    );
}
