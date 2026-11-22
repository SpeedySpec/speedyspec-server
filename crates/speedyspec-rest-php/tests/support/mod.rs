//! Shared by the REST PHP test binaries (each boots PHP once, with its own `main()`).
#![allow(dead_code)]

use std::path::Path;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_operation::identity::MemoryIdentity;
use speedyspec_operation_contracts::Identity;
use speedyspec_operation_php::{FixedSiteKey, OperationArea, SiteId, SiteKey};
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_php_ext::Value as PhpValue;
use speedyspec_rest_php::{RestArea, RestService};
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
    pub service: Rc<RestService>,
    pub php: Php,
    pub container: Rc<Container>,
    pub operations: Rc<OperationArea>,
    pub bindings: Rc<MemoryBindingStore>,
    pub identity: Rc<dyn Identity>,
}

pub fn boot() -> Booted {
    boot_with_key(Rc::new(FixedSiteKey(SiteId {
        blog: 1,
        fingerprint: String::new(),
    })))
}

pub fn boot_with_key(site_key: Rc<dyn SiteKey>) -> Booted {
    let container = Rc::new(Container::new());
    let identity: Rc<dyn Identity> = Rc::new(MemoryIdentity::new().grant(7, "edit_report"));
    let bindings = Rc::new(MemoryBindingStore::new());
    let operations = Rc::new(OperationArea::new(
        container.clone(),
        identity.clone(),
        site_key,
    ));
    let service = Rc::new(RestService::new(
        container.clone(),
        operations.clone(),
        bindings.clone(),
        identity.clone(),
        Rc::new(|| "http://x.test/wp-json".to_owned()),
    ));
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        Rc::new(ContainerArea::new("test", container.clone())),
        operations.clone(),
        Rc::new(RestArea::new(bindings.clone()).with_service(service.clone())),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    Booted {
        service,
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

/// WordPress functions the release's REST files call that need no database, as minimal stand-ins. `_doing_it_wrong`
/// records its messages in `$GLOBALS['__doing_it_wrong']`.
pub const RELEASE_STUBS: &str = r#"
function __( $text, $domain = 'default' ) { return $text; }
function _doing_it_wrong( $function_name, $message, $version ) { $GLOBALS['__doing_it_wrong'][] = $function_name . ': ' . $message; }
function trailingslashit( $value ) { return rtrim( $value, '/\\' ) . '/'; }
function untrailingslashit( $value ) { return rtrim( $value, '/\\' ); }
function is_user_logged_in() { return ( $GLOBALS['uid'] ?? 0 ) > 0; }
function get_current_user_id() { return $GLOBALS['uid'] ?? 0; }
function __return_true() { return true; }
function __return_false() { return false; }
function absint( $maybeint ) { return abs( (int) $maybeint ); }
function wp_is_json_media_type( $media_type ) { return 'application/json' === $media_type || str_ends_with( $media_type, '+json' ); }
function wp_parse_args( $args, $defaults = array() ) { return array_merge( $defaults, is_array( $args ) ? $args : array() ); }
function is_wp_error( $thing ) { return $thing instanceof WP_Error; }
function esc_html( $text ) { return htmlspecialchars( (string) $text, ENT_QUOTES ); }
function wp_json_encode( $value, $flags = 0, $depth = 512 ) { return json_encode( $value, $flags, $depth ); }
function wp_list_filter( $input_list, $args = array(), $operator = 'AND' ) {
    return array_filter( $input_list, static function ( $item ) use ( $args ) {
        foreach ( $args as $key => $value ) {
            if ( ! is_array( $item ) || ! array_key_exists( $key, $item ) || $item[ $key ] != $value ) {
                return false;
            }
        }
        return true;
    } );
}
"#;

/// The pinned WordPress release's root.
pub fn release() -> std::path::PathBuf {
    speedyspec_compat::wordpress_release("7.1.5").expect("the pinned WordPress release")
}

/// Loads the hook functions, the stubs and the release's error, response and request classes and REST functions (not
/// `class-wp-rest-server.php`: the shim replaces it) into the current request.
pub fn load_release(php: &Php) {
    load(php, HOOKS.replace("function absint( $maybeint ) { return abs( (int) $maybeint ); }", "").replace("function wp_is_json_media_type( $media_type ) { return 'application/json' === $media_type || str_ends_with( $media_type, '+json' ); }", "").as_str());
    load(php, RELEASE_STUBS);
    let root = release().join("wp-includes");
    for file in [
        "class-wp-error.php",
        "class-wp-http-response.php",
        "rest-api/class-wp-rest-response.php",
        "rest-api/class-wp-rest-request.php",
        "rest-api.php",
    ] {
        load(
            php,
            &format!("require_once {:?};", root.join(file).display().to_string()),
        );
    }
}
