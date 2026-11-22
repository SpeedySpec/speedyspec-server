//! The PHP adapter: proxies, WP_Error mapping, the hooks, the authenticator and Rest::bind.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_http::{Method, ServerRequestFactory};
use speedyspec_http_message::MessageFactory;
use speedyspec_operation_contracts::{Category, Value};
use speedyspec_php_ext::{PhpObject, Value as PhpValue};
use speedyspec_rest::errors::status_of;
use speedyspec_rest_contracts::{RestHooks, RestMethod, RestRequest, RestResponse, SharedRequest};
use speedyspec_rest_php::{PhpRestAuthenticator, PhpRestHooks, errors};
use speedyspec_transport_contracts::{Authenticator, BindingStore};
use support::*;

const FIXTURE: &str = r#"
function t_pre( $result, $server, $request ) {
    if ( ! empty( $GLOBALS['block'] ) ) {
        return new WP_Error( 'acme_blocked', 'Blocked.', [ 'status' => 403, 'why' => 'ip' ] );
    }
    $GLOBALS['seen']  = $request->get_param( 'id' );
    $GLOBALS['is']    = $request instanceof WP_REST_Request;
    $request->set_param( 'title', 'x' );
    $GLOBALS['via_array'] = $request['title'];
    $GLOBALS['route'] = $request->get_route();
    return null;
}
add_filter( 'rest_pre_dispatch', 't_pre', 10, 3 );
function t_post( $response, $server, $request ) {
    if ( ! empty( $GLOBALS['replace'] ) ) {
        return new WP_REST_Response( [ 'a' => 1 ], 202 );
    }
    $response->header( 'X-A', '1' );
    $response->set_status( 418 );
    $GLOBALS['data_seen'] = $response->get_data();
    return $response;
}
add_filter( 'rest_post_dispatch', 't_post', 10, 3 );
function t_auth( $result ) {
    return empty( $GLOBALS['deny'] ) ? $result : new WP_Error( 'acme_no', 'No.', [ 'status' => 401 ] );
}
add_filter( 'rest_authentication_errors', 't_auth' );
function get_current_user_id() { return $GLOBALS['uid'] ?? 0; }
"#;

fn shared() -> SharedRequest {
    let mut r = RestRequest::new(RestMethod::Get, "/acme/v1/reports/3");
    r.url_params.insert("id".into(), Value::Int(3));
    r.route = Some("/acme/v1/reports/(?P<id>[\\d]+)".into());
    Rc::new(RefCell::new(r))
}

fn global(b: &Booted, name: &str) -> PhpValue {
    eval(&b.php, &format!("return $GLOBALS['{name}'] ?? null;")).unwrap_or(PhpValue::Null)
}

fn main() {
    let b = boot();
    load_wordpress(&b.php);
    load(&b.php, FIXTURE);
    let hooks = PhpRestHooks::new();

    // request_proxy_reads_and_writes_live, proxy_is_a_wp_rest_request
    let request = shared();
    let answered = hooks.pre_dispatch(&request);
    let title = request.borrow().params().get("title").cloned();
    check(
        "request_proxy_reads_and_writes_live",
        answered.is_none()
            && global(&b, "seen") == PhpValue::Int(3)
            && title == Some(Value::from("x"))
            && global(&b, "via_array") == PhpValue::Str("x".into())
            && global(&b, "route") == PhpValue::Str("/acme/v1/reports/3".into()),
        (
            &answered,
            global(&b, "seen"),
            &title,
            global(&b, "via_array"),
            global(&b, "route"),
        ),
    );
    check(
        "proxy_is_a_wp_rest_request",
        global(&b, "is") == PhpValue::Bool(true),
        global(&b, "is"),
    );

    // wp_error_maps_both_ways
    load(&b.php, "$GLOBALS['block'] = true;");
    let blocked = hooks.pre_dispatch(&shared());
    load(&b.php, "$GLOBALS['block'] = false;");
    let error = match blocked {
        Some(Err(e)) => Some(e),
        _ => None,
    };
    let back = error
        .as_ref()
        .map(errors::to_wp_error)
        .and_then(PhpObject::new);
    let back_status = back.as_ref().map(|o| o.call("get_error_data", vec![]));
    check(
        "wp_error_maps_both_ways",
        error.as_ref().is_some_and(|e| {
            e.code == "acme_blocked"
                && e.category == Category::PermissionDenied
                && e.message == "Blocked."
                && *e.details
                    == Value::map([("status", Value::Int(403)), ("why", Value::from("ip"))])
                && status_of(e) == 403
        }) && matches!(&back_status, Some(Ok(PhpValue::Array(items))) if items.iter().any(|(k, v)| *k == speedyspec_php_ext::Key::Str("status".into()) && *v == PhpValue::Int(403))),
        (&error, &back_status),
    );

    // response_proxy_writes_through
    let response = hooks.post_dispatch(
        RestResponse::ok(Value::map([("id", Value::Int(3))])),
        &shared(),
    );
    check(
        "response_proxy_writes_through",
        response.status == 418
            && response.headers.get("X-A").map(String::as_str) == Some("1")
            && response.data == Value::map([("id", Value::Int(3))]),
        (&response, global(&b, "data_seen")),
    );

    // wp_rest_response_maps_back
    load(&b.php, "$GLOBALS['replace'] = true;");
    let replaced = hooks.post_dispatch(RestResponse::ok(Value::Null), &shared());
    load(&b.php, "$GLOBALS['replace'] = false;");
    check(
        "wp_rest_response_maps_back",
        replaced.status == 202 && replaced.data == Value::map([("a", Value::Int(1))]),
        &replaced,
    );

    // authenticator_uses_the_current_user, authentication_errors_map
    load(&b.php, "$GLOBALS['uid'] = 7;");
    let http = MessageFactory
        .create_server_request(Method::Get, "http://x.test/wp-json/", vec![])
        .unwrap();
    let caller = PhpRestAuthenticator::new(b.identity.clone()).authenticate(&http);
    load(&b.php, "$GLOBALS['uid'] = 0;");
    let anonymous = PhpRestAuthenticator::new(b.identity.clone()).authenticate(&http);
    check(
        "authenticator_uses_the_current_user",
        caller
            .as_ref()
            .is_ok_and(|c| c.user == Some(7) && c.transport == "rest")
            && anonymous.as_ref().is_ok_and(|c| c.user.is_none()),
        (caller.map(|c| c.user), anonymous.map(|c| c.user)),
    );
    load(&b.php, "$GLOBALS['deny'] = true;");
    let denied = hooks.authentication_errors();
    load(&b.php, "$GLOBALS['deny'] = false;");
    check(
        "authentication_errors_map",
        matches!(&denied, Err(e) if e.category == Category::Unauthenticated && e.code == "acme_no")
            && hooks.authentication_errors().is_ok(),
        &denied,
    );

    // rest_bind_stores_a_checked_binding
    let bound = eval(
        &b.php,
        r"\SpeedySpec\Rest\Rest::bind( 'acme/x' )->route( 'acme/v2', '/x' )->methods( 'GET' ); return true;",
    );
    let stored = b.bindings.binding("acme/x", "rest");
    let bad = eval(
        &b.php,
        r"try { \SpeedySpec\Rest\Rest::bind( 'acme/y' )->route( 'bad', '/x' )->methods( 'GET' ); return 'ran'; }
          catch ( \SpeedySpec\Operation\RegistrationException $e ) { return $e->getMessage(); }",
    );
    check(
        "rest_bind_stores_a_checked_binding",
        bound.is_ok()
            && stored
                == Some(Value::map([
                    ("namespace", Value::from("acme/v2")),
                    ("route", Value::from("/x")),
                    ("methods", Value::List(vec![Value::from("GET")])),
                ]))
            && matches!(&bad, Ok(PhpValue::Str(m)) if m.contains("vendor/version")),
        (&bound, &stored, &bad),
    );

    // a_stale_handle_throws
    let stale = eval(
        &b.php,
        r"try { \SpeedySpec\Rest\Native\request_get( 999999, 'method' ); return 'ran'; } catch ( \LogicException $e ) { return 'threw'; }",
    );
    check(
        "a_stale_handle_throws",
        matches!(&stale, Ok(PhpValue::Str(s)) if s == "threw"),
        &stale,
    );
}
