//! Shared by the REST PHP test binaries (each boots PHP once, with its own `main()`).
#![allow(dead_code)]

use std::path::Path;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_operation::identity::MemoryIdentity;
use speedyspec_operation_contracts::Identity;
use speedyspec_operation_php::{FixedSiteKey, OperationArea, SiteId};
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_php_ext::Value as PhpValue;
use speedyspec_rest_php::RestArea;
use speedyspec_transport::MemoryBindingStore;

/// WordPress's hook functions, minimal (filters with accepted-args counts, actions, `did_action`), and the helpers
/// WordPress's classes call: `absint` (`WP_HTTP_Response::set_status`), `wp_is_json_media_type`
/// (`WP_REST_Request::is_json_content_type`).
pub const HOOKS: &str = r#"
function add_filter( string $hook, callable $callback, int $priority = 10, int $accepted_args = 1 ): bool {
    $GLOBALS['__filters'][ $hook ][] = [ $callback, $accepted_args ];
    return true;
}
function apply_filters( string $hook, mixed $value, mixed ...$args ): mixed {
    foreach ( $GLOBALS['__filters'][ $hook ] ?? [] as [ $callback, $accepted ] ) {
        $value = $callback( $value, ...array_slice( $args, 0, max( 0, $accepted - 1 ) ) );
    }
    return $value;
}
function add_action( string $hook, callable $callback, int $priority = 10, int $accepted_args = 1 ): bool {
    return add_filter( $hook, $callback, $priority, $accepted_args );
}
function do_action( string $hook, mixed ...$args ): void {
    $GLOBALS['__did'][ $hook ] = ( $GLOBALS['__did'][ $hook ] ?? 0 ) + 1;
    foreach ( $GLOBALS['__filters'][ $hook ] ?? [] as [ $callback, $accepted ] ) {
        $callback( ...array_slice( $args, 0, $accepted ) );
    }
}
function did_action( string $hook ): int { return $GLOBALS['__did'][ $hook ] ?? 0; }
function absint( $maybeint ) { return abs( (int) $maybeint ); }
function wp_is_json_media_type( $media_type ) { return 'application/json' === $media_type || str_ends_with( $media_type, '+json' ); }
"#;

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
    pub container: Rc<Container>,
    pub operations: Rc<OperationArea>,
    pub bindings: Rc<MemoryBindingStore>,
    pub identity: Rc<dyn Identity>,
}

pub fn boot() -> Booted {
    let container = Rc::new(Container::new());
    let identity: Rc<dyn Identity> = Rc::new(MemoryIdentity::new().grant(7, "edit_report"));
    let bindings = Rc::new(MemoryBindingStore::new());
    let operations = Rc::new(OperationArea::new(
        container.clone(),
        identity.clone(),
        Rc::new(FixedSiteKey(SiteId {
            blog: 1,
            fingerprint: String::new(),
        })),
    ));
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        Rc::new(ContainerArea::new("test", container.clone())),
        operations.clone(),
        Rc::new(RestArea::new(bindings.clone())),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    Booted {
        php,
        container,
        operations,
        bindings,
        identity,
    }
}

/// Loads the hook functions and WordPress's error, response and request classes into the current request.
pub fn load_wordpress(php: &Php) {
    load(php, HOOKS);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/wordpress");
    for file in [
        "class-wp-error.php",
        "class-wp-http-response.php",
        "class-wp-rest-response.php",
        "class-wp-rest-request.php",
    ] {
        load(
            php,
            &format!("require_once {:?};", dir.join(file).display().to_string()),
        );
    }
}

pub fn load(php: &Php, code: &str) {
    if let Err(err) = php.eval("fixture", code) {
        panic!("the fixture failed to load: {err:?}");
    }
}

pub fn eval(php: &Php, code: &str) -> Result<PhpValue, PhpError> {
    php.eval("t", code)
}
