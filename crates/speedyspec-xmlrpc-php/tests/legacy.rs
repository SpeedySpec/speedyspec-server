//! The wp_xmlrpc_server shim: WordPress's and plugins' methods as legacy operations (LegacyHandlerAdapter), and
//! `wp_xmlrpc_server::call()` reaching every method (LegacyCallerAdapter).

mod support;

use std::rc::Rc;

use speedyspec_operation_contracts::{Caller, OperationError, Value};
use speedyspec_operation_dispatch::{OperationHandler, bind_handler};
use speedyspec_php_ext::Value as PhpValue;
use support::*;

/// WordPress's user functions, minimal: `admin`/`pw` is user 7.
pub const USERS: &str = r#"
class WP_User { public $ID; public function __construct( $id ) { $this->ID = $id; } }
function wp_authenticate( $username, $password ) {
    $GLOBALS['authenticated'][] = $username;
    return ( 'admin' === $username && 'pw' === $password ) ? new WP_User( 7 ) : new WP_Error( 'invalid_username', 'Unknown.' );
}
function wp_set_current_user( $id ) { $GLOBALS['current_user'] = $id; }
function get_userdata( $id ) { return new WP_User( $id ); }
"#;

pub const PLUGIN: &str = r#"
use SpeedySpec\Operation\{S, Operation, Handler, Caller};
use SpeedySpec\XmlRpc\XmlRpc;

function acme_hello( array $input, Caller $caller ): array { return [ 'greeting' => 'hi ' . ( $input['name'] ?? '' ) ]; }
function acme_secure( array $input, Caller $caller ): array { return [ 'user' => $caller->user ]; }
function acme_init(): void {
    Operation::query( 'acme/hello' )->input( null, S::object( [ 'name' => S::string() ] ) )->output( null, S::any() )->public()->register();
    Handler::for( 'acme/hello', 'acme_hello' );
    XmlRpc::bind( 'acme/hello' )->method( 'acme.hello' )->params( 'name' );
    Operation::query( 'acme/secure' )->input( null, S::object( [] ) )->output( null, S::any() )->public()->register();
    Handler::for( 'acme/secure', 'acme_secure' );
    XmlRpc::bind( 'acme/secure' )->method( 'acme.secure' )->params( 'username', 'password' );
    Operation::query( 'acme/when' )->input( null, S::object( [] ) )->output( null, S::any() )->public()->register();
    XmlRpc::bind( 'acme/when' )->method( 'acme.when' )->params();
}
add_action( 'wp_operations_init', 'acme_init' );

function acme_old( $args ) { return [ 'got' => $args ]; }
function acme_teapot( $args ) { return new IXR_Error( 418, 'Teapot' ); }
final class Acme_Override { public static function blogs( $args ) { return 'overridden'; } }
function acme_methods( $methods ) {
    $GLOBALS['saw_operation_method'] = ( $methods['acme.hello'] ?? null ) instanceof SpeedySpec\XmlRpc\OperationMethod;
    $methods['acme.old']         = 'acme_old';
    $methods['acme.teapot']      = 'acme_teapot';
    $methods['acme.missing']     = 'acme_not_a_function';
    $methods['wp.getUsersBlogs'] = [ 'Acme_Override', 'blogs' ];
    unset( $methods['pingback.ping'] );
    return $methods;
}
add_filter( 'xmlrpc_methods', 'acme_methods' );
"#;

struct When;

impl OperationHandler for When {
    fn handle(
        &self,
        _: Box<dyn std::any::Any>,
        _: &Caller,
    ) -> Result<Box<dyn std::any::Any>, OperationError> {
        unreachable!()
    }
    fn neutral(&self) -> bool {
        true
    }
    fn handle_neutral(&self, _: &Value, _: &Caller) -> Result<Value, OperationError> {
        Ok(Value::DateTime(
            chrono::NaiveDate::from_ymd_opt(2026, 11, 23)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap(),
        ))
    }
}

fn text(b: &Booted, code: &str) -> String {
    match eval(&b.php, code) {
        Ok(PhpValue::Str(s)) => s,
        other => format!("{other:?}"),
    }
}

fn main() {
    let b = boot();
    load_wordpress(&b.php);
    load(&b.php, USERS);
    load(&b.php, PLUGIN);
    let when: Rc<dyn OperationHandler> = Rc::new(When);
    bind_handler(&b.container, "acme/when", move |_, _| Ok(when.clone()));
    load(&b.php, "$GLOBALS['s'] = new wp_xmlrpc_server();");

    let five = eval(
        &b.php,
        "return $GLOBALS['s']->call( 'demo.addTwoNumbers', [ 2, 3 ] );",
    );
    let hello = eval(&b.php, "return $GLOBALS['s']->call( 'demo.sayHello', [] );");
    check(
        "wordpress_core_methods_are_served",
        matches!(five, Ok(PhpValue::Int(5)))
            && matches!(&hello, Ok(PhpValue::Str(s)) if s == "Hello!"),
        (&five, &hello),
    );

    let old = text(
        &b,
        "return json_encode( $GLOBALS['s']->call( 'acme.old', [ 'x', 'y' ] ) );",
    );
    let restricted = speedyspec_xmlrpc_php::legacy::operations()
        .into_iter()
        .find(|spec| spec.name.as_str().starts_with("legacy-xmlrpc/acme-old-"))
        .and_then(|spec| spec.transports);
    check(
        "a_plugin_method_is_served_and_restricted_to_xmlrpc",
        old == r#"{"got":["x","y"]}"#
            && restricted.is_some_and(|t| t.len() == 1 && t.contains("xmlrpc")),
        &old,
    );

    let solo = text(
        &b,
        "return json_encode( $GLOBALS['s']->call( 'acme.old', [ 'solo' ] ) );",
    );
    check(
        "a_single_param_reaches_a_legacy_method_unwrapped",
        solo == r#"{"got":"solo"}"#,
        &solo,
    );

    let overridden = text(&b, "return $GLOBALS['s']->call( 'wp.getUsersBlogs', [] );");
    check(
        "a_plugin_can_override_a_core_method",
        overridden == "overridden",
        &overridden,
    );

    let hidden = text(
        &b,
        "$r = $GLOBALS['s']->call( 'pingback.ping', [ 'a', 'b' ] );
         $listed = $GLOBALS['s']->call( 'system.listMethods', [] );
         return ( $r instanceof IXR_Error ? $r->code : 'no error' ) . ':' . ( in_array( 'pingback.ping', $listed, true ) ? 'listed' : 'unlisted' ) . ':' . ( in_array( 'demo.sayHello', $listed, true ) ? 'core' : 'no core' );",
    );
    check(
        "unset_pingback_ping_hides_it",
        hidden == "-32601:unlisted:core",
        &hidden,
    );

    let teapot = text(
        &b,
        "$r = $GLOBALS['s']->call( 'acme.teapot', [] ); return $r instanceof IXR_Error ? $r->code . ' ' . $r->message : 'no error';",
    );
    check(
        "an_ixr_error_keeps_its_fault_code",
        teapot == "418 Teapot",
        &teapot,
    );

    let missing = text(
        &b,
        "$r = $GLOBALS['s']->call( 'acme.missing', [] ); return $r instanceof IXR_Error ? $r->code . ' ' . $r->message : 'no error';",
    );
    check(
        "a_missing_function_is_ixrs_fault",
        missing
            == "-32601 server error. requested function &quot;acme_not_a_function&quot; does not exist.",
        &missing,
    );

    let invoked = text(
        &b,
        "$m = $GLOBALS['s']->methods['acme.hello']; return json_encode( [ $GLOBALS['saw_operation_method'], $m( 'bob' ) ] );",
    );
    check(
        "operations_appear_in_the_filtered_table_as_operation_methods",
        invoked == r#"[true,{"greeting":"hi bob"}]"#,
        &invoked,
    );

    let called = text(
        &b,
        "$hello = $GLOBALS['s']->call( 'acme.hello', [ 'ann' ] ); $when = $GLOBALS['s']->call( 'acme.when', [] );
         return json_encode( [ $hello, $when instanceof IXR_Date ? $when->getIso() : $when ] );",
    );
    check(
        "call_reaches_an_operation_and_a_legacy_method",
        called == r#"[{"greeting":"hi ann"},"20261123T10:00:00"]"#,
        &called,
    );

    let signed_in = text(
        &b,
        "return json_encode( $GLOBALS['s']->call( 'acme.secure', [ 'admin', 'pw' ] ) );",
    );
    check(
        "an_operation_logs_in_with_wordpress",
        signed_in == r#"{"user":7}"# && global(&b.php, "current_user") == PhpValue::Int(7),
        &signed_in,
    );

    load(
        &b.php,
        "$GLOBALS['authenticated'] = []; $GLOBALS['s'] = new wp_xmlrpc_server();",
    );
    let latched = text(
        &b,
        "$r = $GLOBALS['s']->call( 'system.multicall', [ [
             [ 'methodName' => 'wp.getOptions', 'params' => [ 1, 'eve', 'x' ] ],
             [ 'methodName' => 'acme.secure', 'params' => [ 'admin', 'pw' ] ],
         ] ] );
         return json_encode( [ $r, $GLOBALS['authenticated'] ] );",
    );
    check(
        "a_failed_login_in_a_legacy_method_blocks_an_operation_login_later_in_the_multicall",
        latched
            == r#"[[{"faultCode":403,"faultString":"Incorrect username or password."},{"faultCode":403,"faultString":"Incorrect username or password."}],["eve"]]"#,
        &latched,
    );

    load(
        &b.php,
        "add_filter( 'xmlrpc_enabled', '__return_false' ); $GLOBALS['s'] = new wp_xmlrpc_server();",
    );
    let disabled = text(
        &b,
        "$r = $GLOBALS['s']->wp_getOptions( [ 1, 'admin', 'pw' ] ); return $r instanceof IXR_Error ? (string) $r->code : 'no error';",
    );
    check(
        "a_new_server_is_disabled_by_the_filter",
        disabled == "405",
        &disabled,
    );
}
