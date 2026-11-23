//! Shared by the XML-RPC PHP test binaries (each boots PHP once, with its own `main()`).
#![allow(dead_code)]

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_operation::identity::MemoryIdentity;
use speedyspec_operation_contracts::Identity;
use speedyspec_operation_php::{FixedSiteKey, OperationArea, SiteId};
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_php_ext::Value as PhpValue;
use speedyspec_transport::MemoryBindingStore;
use speedyspec_xmlrpc_php::XmlRpcArea;

/// WordPress's hook functions, minimal (filters with accepted-args counts and priorities, actions, removal).
pub const HOOKS: &str = r#"
function add_filter( string $hook, callable $callback, int $priority = 10, int $accepted_args = 1 ): bool {
    $GLOBALS['__filters'][ $hook ][ $priority ][] = [ $callback, $accepted_args ];
    ksort( $GLOBALS['__filters'][ $hook ] );
    return true;
}
function has_filter( string $hook, $callback = false, $priority = false ) {
    foreach ( $GLOBALS['__filters'][ $hook ] ?? [] as $p => $list ) {
        foreach ( $list as [ $cb ] ) {
            if ( $cb === $callback ) {
                return $p;
            }
        }
    }
    return false;
}
function remove_filter( string $hook, $callback, int $priority = 10 ): bool {
    foreach ( $GLOBALS['__filters'][ $hook ][ $priority ] ?? [] as $i => [ $cb ] ) {
        if ( $cb === $callback ) {
            unset( $GLOBALS['__filters'][ $hook ][ $priority ][ $i ] );
            return true;
        }
    }
    return false;
}
function apply_filters( string $hook, mixed $value, mixed ...$args ): mixed {
    foreach ( $GLOBALS['__filters'][ $hook ] ?? [] as $list ) {
        foreach ( $list as [ $callback, $accepted ] ) {
            $value = $callback( $value, ...array_slice( $args, 0, max( 0, $accepted - 1 ) ) );
        }
    }
    return $value;
}
function add_action( string $hook, callable $callback, int $priority = 10, int $accepted_args = 1 ): bool {
    return add_filter( $hook, $callback, $priority, $accepted_args );
}
function do_action( string $hook, mixed ...$args ): void {
    foreach ( $GLOBALS['__filters'][ $hook ] ?? [] as $list ) {
        foreach ( $list as [ $callback, $accepted ] ) {
            $callback( ...array_slice( $args, 0, $accepted ) );
        }
    }
}
function __( $text, $domain = 'default' ) { return $text; }
function __return_true() { return true; }
function __return_false() { return false; }
function is_wp_error( $thing ) { return $thing instanceof WP_Error; }
function _doing_it_wrong( $function_name, $message, $version ) { $GLOBALS['__doing_it_wrong'][] = $function_name . ': ' . $message; }
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
        Rc::new(XmlRpcArea::new(bindings.clone())),
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

/// The pinned WordPress release's root.
pub fn release() -> std::path::PathBuf {
    speedyspec_compat::wordpress_release("7.1.5").expect("the pinned WordPress release")
}

/// Loads the hook functions, `WP_Error` and the release's IXR classes into the current request.
pub fn load_wordpress(php: &Php) {
    load(php, HOOKS);
    let root = release();
    load(
        php,
        &format!(
            "if ( ! defined( 'ABSPATH' ) ) {{ define( 'ABSPATH', {:?} ); define( 'WPINC', 'wp-includes' ); }}",
            format!("{}/", root.display())
        ),
    );
    for file in ["class-wp-error.php", "class-IXR.php"] {
        load(
            php,
            &format!(
                "require_once {:?};",
                root.join("wp-includes").join(file).display().to_string()
            ),
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

pub fn global(php: &Php, name: &str) -> PhpValue {
    eval(php, &format!("return $GLOBALS[{name:?}] ?? null;")).unwrap_or(PhpValue::Null)
}
