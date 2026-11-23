//! The PHP adapters: WordPress's XML-RPC hooks, login and IXR value conversion.

mod support;

use chrono::NaiveDate;
use speedyspec_operation_contracts::{Category, Value};
use speedyspec_php_ext::Value as PhpValue;
use speedyspec_xmlrpc_contracts::{Fault, XmlRpcHooks, XmlRpcLogin};
use speedyspec_xmlrpc_php::{PhpXmlRpcHooks, PhpXmlRpcLogin, convert};
use support::*;

const FIXTURE: &str = r#"
class WP_User { public $ID; public function __construct( $id ) { $this->ID = $id; } }
function wp_authenticate( $username, $password ) {
    $GLOBALS['authenticated'][] = $username;
    return ( 'admin' === $username && 'pw' === $password ) ? new WP_User( 7 ) : new WP_Error( 'invalid_username', 'Unknown.' );
}
function wp_set_current_user( $id ) { $GLOBALS['current_user'] = $id; }
function t_limit( $limit ) { return empty( $GLOBALS['limit'] ) ? $limit : $GLOBALS['limit']; }
add_filter( 'xmlrpc_element_limit', 't_limit' );
function t_login_error( $error, $user ) {
    $GLOBALS['login_error_user_is_wp_error'] = $user instanceof WP_Error;
    return empty( $GLOBALS['custom_login'] ) ? $error : new IXR_Error( 403, 'Custom: ' . $error->message );
}
add_filter( 'xmlrpc_login_error', 't_login_error', 10, 2 );
function t_call( $method, $args, $server ) {
    if ( ! empty( $GLOBALS['explode'] ) ) {
        throw new RuntimeException( 'a security plugin broke' );
    }
    $GLOBALS['called'][] = [ $method, $args ];
}
add_action( 'xmlrpc_call', 't_call', 10, 3 );
function t_explode_enabled( $enabled ) {
    if ( ! empty( $GLOBALS['explode'] ) ) {
        throw new RuntimeException( 'a security plugin broke' );
    }
    return $enabled;
}
add_filter( 'xmlrpc_enabled', 't_explode_enabled' );
function t_inspect( $value ) {
    return [
        'date'   => $value['d'] instanceof IXR_Date ? $value['d']->getIso() : 'not a date',
        'bytes'  => $value['b'] instanceof IXR_Base64 ? $value['b']->data : 'not base64',
        'list'   => $value['l'],
        'map'    => $value['m'],
        'null'   => $value['n'],
    ];
}
"#;

fn at(y: i32, m: u32, d: u32, h: u32) -> Value {
    Value::DateTime(
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap(),
    )
}

fn main() {
    let b = boot();
    load_wordpress(&b.php);
    load(&b.php, FIXTURE);
    let hooks = PhpXmlRpcHooks::new();

    check("enabled_by_default", hooks.enabled(), ());

    load(
        &b.php,
        "add_filter( 'pre_option_enable_xmlrpc', '__return_true' ); add_filter( 'xmlrpc_enabled', '__return_false', 20 );",
    );
    check(
        "enabled_follows_wordpress_filter_order",
        !hooks.enabled(),
        (),
    );
    load(
        &b.php,
        "unset( $GLOBALS['__filters']['pre_option_enable_xmlrpc'], $GLOBALS['__filters']['xmlrpc_enabled'][20] );
         add_filter( 'option_enable_xmlrpc', '__return_false' );",
    );
    check("option_enable_xmlrpc_applies", !hooks.enabled(), ());
    load(
        &b.php,
        "add_filter( 'pre_option_enable_xmlrpc', '__return_true' );",
    );
    check("pre_option_skips_option_enable_xmlrpc", hooks.enabled(), ());

    check(
        "element_limit_default",
        hooks.element_limit(30000) == 30000,
        hooks.element_limit(30000),
    );
    load(&b.php, "$GLOBALS['limit'] = 5;");
    check(
        "element_limit_filter_applies",
        hooks.element_limit(30000) == 5,
        hooks.element_limit(30000),
    );

    let fault = hooks.login_error(Fault::new(403, "Incorrect username or password."), "eve");
    check(
        "login_error_unchanged_by_default",
        fault == Fault::new(403, "Incorrect username or password.")
            && global(&b.php, "login_error_user_is_wp_error") == PhpValue::Bool(true),
        &fault,
    );
    load(&b.php, "$GLOBALS['custom_login'] = true;");
    let fault = hooks.login_error(Fault::new(403, "Incorrect username or password."), "eve");
    check(
        "login_error_filter_changes_the_fault",
        fault == Fault::new(403, "Custom: Incorrect username or password."),
        &fault,
    );

    let called = hooks.call("acme.ping", &[Value::Int(1), Value::from("admin")]);
    let seen = eval(&b.php, "return json_encode( $GLOBALS['called'] ?? null );").unwrap();
    check(
        "call_fires_xmlrpc_call",
        called.is_ok() && seen == PhpValue::Str(r#"[["acme.ping",[1,"admin"]]]"#.into()),
        (&called, &seen),
    );

    load(&b.php, "$GLOBALS['explode'] = true;");
    let call_failed = hooks.call("acme.ping", &[]);
    check(
        "a_throwing_filter_fails_closed",
        !hooks.enabled()
            && call_failed
                .as_ref()
                .is_err_and(|e| e.category == Category::Internal),
        &call_failed,
    );
    load(&b.php, "$GLOBALS['explode'] = false;");

    let login = PhpXmlRpcLogin::new(b.identity.clone());
    let caller = login.authenticate("admin", "pw");
    check(
        "login_sets_the_current_user",
        caller
            .as_ref()
            .is_ok_and(|c| c.user == Some(7) && c.transport == "xmlrpc")
            && global(&b.php, "current_user") == PhpValue::Int(7),
        caller.as_ref().map(|c| c.user).map_err(|e| e.clone()),
    );
    let refused = login.authenticate("eve", "x");
    check(
        "a_wp_error_is_unauthenticated",
        refused
            .as_ref()
            .is_err_and(|e| e.category == Category::Unauthenticated),
        refused.as_ref().map(|c| c.user).map_err(|e| e.clone()),
    );

    let php = eval(
        &b.php,
        "return [ 'd' => new IXR_Date( '20261123T10:00:00' ), 'b' => new IXR_Base64( 'hi' ), 'l' => [ 1, 2 ],
                  'm' => [ '0' => 1, '2' => 2 ], 'o' => (object) [ 'a' => 1 ], 'n' => null, 'f' => 1.5, 's' => 'x' ];",
    )
    .unwrap();
    let neutral = convert::to_neutral(&php);
    let expected = Value::map([
        ("d", at(2026, 11, 23, 10)),
        ("b", Value::Binary(b"hi".to_vec())),
        ("l", Value::List(vec![Value::Int(1), Value::Int(2)])),
        (
            "m",
            Value::map([("0", Value::Int(1)), ("2", Value::Int(2))]),
        ),
        ("o", Value::map([("a", Value::Int(1))])),
        ("n", Value::Null),
        ("f", Value::Float(1.5)),
        ("s", Value::from("x")),
    ]);
    check("ixr_values_to_neutral", neutral == expected, &neutral);
    let back = convert::to_php(&expected);
    let inspected = speedyspec_php::call(&PhpValue::Str("t_inspect".into()), &[back]).unwrap();
    let inspected = speedyspec_operation_php::convert::from_php(&inspected).unwrap();
    check(
        "neutral_values_to_ixr",
        inspected
            == Value::map([
                ("date", Value::from("20261123T10:00:00")),
                ("bytes", Value::from("hi")),
                ("list", Value::List(vec![Value::Int(1), Value::Int(2)])),
                (
                    "map",
                    Value::map([("0", Value::Int(1)), ("2", Value::Int(2))]),
                ),
                ("null", Value::Null),
            ]),
        &inspected,
    );
}
