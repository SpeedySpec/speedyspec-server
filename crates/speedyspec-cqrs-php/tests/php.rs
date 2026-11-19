//! The buses from PHP, and PHP handlers overriding Rust ones for a request.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`).

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_cqrs::{
    Command, CommandBus, CommandHandler, HandlerRef, Query, QueryBus, QueryHandler,
};
use speedyspec_cqrs_php::{CqrsArea, php_command, php_query};
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_php_ext::{FromPhp, ToPhp, Value, boundary_crossings};

const GREET: &str = "SpeedySpec\\Testing\\GreetHandler";
const COUNT: &str = "SpeedySpec\\Testing\\CountHandler";

struct Greet(String);

impl Command for Greet {
    type Result = String;
    const KEY: &'static str = GREET;
}

impl ToPhp for Greet {
    fn to_php(self) -> Value {
        Value::Str(self.0)
    }
}

impl FromPhp for Greet {
    fn from_php(value: Value) -> Result<Self, String> {
        String::from_php(value).map(Greet)
    }
}

struct Count;

impl Query for Count {
    type Result = i64;
    const KEY: &'static str = COUNT;
}

impl ToPhp for Count {
    fn to_php(self) -> Value {
        Value::Null
    }
}

impl FromPhp for Count {
    fn from_php(_value: Value) -> Result<Self, String> {
        Ok(Count)
    }
}

struct RustGreet;

impl CommandHandler<Greet> for RustGreet {
    fn handle(&self, command: Greet) -> String {
        format!("hello {}", command.0)
    }
}

struct RustCount;

impl QueryHandler<Count> for RustCount {
    fn handle(&self, _query: Count) -> i64 {
        1
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

fn is(result: &Result<Value, PhpError>, expected: Value) -> bool {
    matches!(result, Ok(value) if *value == expected)
}

fn main() {
    let container = Rc::new(Container::new());
    speedyspec_cqrs::bind_command_handler::<Greet>(&container, |_, _| Ok(Rc::new(RustGreet)));
    speedyspec_cqrs::bind_query_handler::<Count>(&container, |_, _| Ok(Rc::new(RustCount)));
    let commands = Rc::new(CommandBus::new(container.clone()));
    let queries = Rc::new(QueryBus::new(container.clone()));

    let area = php_query::<Count>(php_command::<Greet>(ContainerArea::new(
        "demo",
        container.clone(),
    )));
    let cqrs = CqrsArea::new(commands.clone(), queries.clone())
        .command::<Greet>()
        .query::<Count>();
    let areas: Vec<Rc<dyn NativeArea>> = vec![Rc::new(area), Rc::new(cqrs)];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let direct = HandlerRef::<Greet>::new(container.clone());

    // rust_handler_never_enters_php
    let before = boundary_crossings();
    let greeting = commands.dispatch(Greet("ada".into())).unwrap();
    let counted = queries.ask(Count).unwrap();
    let directly = direct.call(Greet("cy".into())).unwrap();
    check(
        "rust_handler_never_enters_php",
        greeting == "hello ada"
            && counted == 1
            && directly == "hello cy"
            && boundary_crossings() == before,
        (greeting, counted, directly, boundary_crossings() - before),
    );

    // php_dispatches_through_the_bus
    let from_php = php.eval(
        "t",
        r"return \SpeedySpec\Cqrs\CommandBus::dispatch('SpeedySpec\Testing\GreetHandler', 'bo');",
    );
    check(
        "php_dispatches_through_the_bus",
        is(&from_php, Value::Str("hello bo".into())),
        &from_php,
    );
    let asked = php.eval(
        "t",
        r"return \SpeedySpec\Cqrs\QueryBus::ask('SpeedySpec\Testing\CountHandler');",
    );
    check(
        "php_asks_through_the_bus",
        is(&asked, Value::Int(1)),
        &asked,
    );

    // php_handler_overrides_rust_handler
    php.eval(
        "t",
        r#"final class PhpGreet { public function handle(string $name): string { return "hi $name"; } }
          final class PhpCount { public function handle(mixed $query): int { return 7; } }
          $c = \SpeedySpec\Container\Container::getInstance();
          $c->bind('SpeedySpec\Testing\GreetHandler', PhpGreet::class);
          $c->bind('SpeedySpec\Testing\CountHandler', PhpCount::class);"#,
    )
    .expect("PHP binds the handlers");
    let greeting = commands.dispatch(Greet("ada".into())).unwrap();
    let directly = direct.call(Greet("cy".into())).unwrap();
    let counted = queries.ask(Count).unwrap();
    check(
        "php_handler_overrides_rust_handler",
        greeting == "hi ada" && directly == "hi cy" && counted == 7,
        (greeting, directly, counted),
    );
    let from_php = php.eval(
        "t",
        r"return \SpeedySpec\Cqrs\CommandBus::dispatch('SpeedySpec\Testing\GreetHandler', 'bo');",
    );
    check(
        "php_dispatch_reaches_the_php_handler",
        is(&from_php, Value::Str("hi bo".into())),
        &from_php,
    );
    let unknown = php.eval(
        "t",
        r"try { \SpeedySpec\Cqrs\CommandBus::dispatch('nothing', 1); return 'ran'; }
          catch (\SpeedySpec\Cqrs\BusException $e) { return $e->getMessage(); }",
    );
    check(
        "php_dispatch_of_an_unknown_command_throws",
        matches!(&unknown, Ok(Value::Str(m)) if m.contains("nothing")),
        &unknown,
    );

    // the override ends with the request
    php.restart_request().expect("the next request starts");
    let before = boundary_crossings();
    let greeting = commands.dispatch(Greet("ada".into())).unwrap();
    let directly = direct.call(Greet("cy".into())).unwrap();
    check(
        "rust_handler_is_back_next_request",
        greeting == "hello ada" && directly == "hello cy" && boundary_crossings() == before,
        (greeting, directly),
    );
}
