//! Shared by the PHP test binaries (each boots PHP once, with its own `main()`).
#![allow(dead_code)]

use std::rc::Rc;

use serde_json::Value as Json;
use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_operation::identity::MemoryIdentity;
use speedyspec_operation_contracts::{Identity, Value};
use speedyspec_operation_php::{OperationArea, SiteKey};
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_php_ext::Value as PhpValue;

/// A minimal `add_action`/`do_action`/`did_action`, as WordPress provides them.
pub const HOOKS: &str = r#"
function add_action( string $hook, callable $callback ): void { $GLOBALS['__actions'][ $hook ][] = $callback; }
function do_action( string $hook, mixed ...$args ): void {
    $GLOBALS['__did'][ $hook ] = ( $GLOBALS['__did'][ $hook ] ?? 0 ) + 1;
    foreach ( $GLOBALS['__actions'][ $hook ] ?? [] as $callback ) { $callback( ...$args ); }
}
function did_action( string $hook ): int { return $GLOBALS['__did'][ $hook ] ?? 0; }
"#;

/// WordPress has loaded its plugins: operations may initialize from here on.
pub const READY: &str = "do_action( 'init' );";

pub fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

pub struct Booted {
    pub php: Php,
    pub area: Rc<OperationArea>,
    pub container: Rc<Container>,
}

pub fn boot(identity: Rc<dyn Identity>, site_key: Rc<dyn SiteKey>) -> Booted {
    let container = Rc::new(Container::new());
    let container_area = ContainerArea::new("test", container.clone());
    let area = Rc::new(OperationArea::new(container.clone(), identity, site_key));
    let areas: Vec<Rc<dyn NativeArea>> = vec![Rc::new(container_area), area.clone()];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    Booted {
        php,
        area,
        container,
    }
}

pub fn identity() -> Rc<dyn Identity> {
    Rc::new(MemoryIdentity::new().grant(7, "edit_report"))
}

/// Evaluates `code` (which must load cleanly), panicking with PHP's error otherwise.
pub fn load(php: &Php, code: &str) {
    if let Err(err) = php.eval("fixture", code) {
        panic!("the fixture failed to load: {err:?}");
    }
}

pub fn eval(php: &Php, code: &str) -> Result<PhpValue, PhpError> {
    php.eval("t", code)
}

pub fn neutral(value: &PhpValue) -> Value {
    speedyspec_operation_php::convert::from_php(value).expect("a plain PHP value")
}

pub fn json(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(i) => Json::from(*i),
        Value::Float(f) => serde_json::Number::from_f64(*f).map_or(Json::Null, Json::Number),
        Value::String(s) => Json::String(s.clone()),
        Value::List(items) => Json::Array(items.iter().map(json).collect()),
        Value::Map(map) => Json::Object(map.iter().map(|(k, v)| (k.clone(), json(v))).collect()),
        other => panic!("no JSON form for {other:?}"),
    }
}
