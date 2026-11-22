//! register_rest_route through the WP_REST_Server shim: legacy routes as rest-only operations.

mod support;

use serde_json::Value as Json;
use speedyspec_http::{Message, Method, Response, ServerRequestFactory};
use speedyspec_http_message::MessageFactory;
use speedyspec_operation_contracts::OperationCatalog;
use speedyspec_php_ext::Value as PhpValue;
use speedyspec_rest::RouteMatch;
use speedyspec_rest_contracts::RestMethod;
use support::*;

const PLUGIN: &str = r#"
function legacy_routes( $server ) {
    $label = $GLOBALS['label'] ?? 'one';
    register_rest_route( 'acme/v1', '/r/(?P<id>\d+)', [
        'methods'             => 'GET',
        'callback'            => static fn ( $request ) => [ 'id' => (int) $request['id'], 'label' => $label ],
        'permission_callback' => '__return_true',
    ] );
    register_rest_route( 'acme/v1', '/dup', [ 'methods' => 'GET', 'callback' => static fn () => 'first', 'permission_callback' => '__return_true' ] );
    register_rest_route( 'acme/v1', '/dup', [ 'methods' => 'GET', 'callback' => static fn () => 'second', 'permission_callback' => '__return_true' ] );
    register_rest_route( 'oembed/1.0', '/over', [ 'methods' => 'GET', 'callback' => static fn () => 'core', 'permission_callback' => '__return_true' ] );
    register_rest_route( 'oembed/1.0', '/over', [ 'methods' => 'GET', 'callback' => static fn () => 'plugin', 'permission_callback' => '__return_true' ], true );
    $public = static fn ( $request ) => true;
    if ( ! empty( $GLOBALS['only_b'] ) ) {
        register_rest_route( 'acme/v1', '/b', [ 'methods' => 'GET', 'callback' => static fn () => 'b', 'permission_callback' => '__return_true' ] );
        return;
    }
    register_rest_route( 'acme/v1', '/swap', [ 'methods' => 'GET', 'callback' => static fn () => 'get', 'permission_callback' => '__return_true' ] );
    register_rest_route( 'acme/v1', '/swap', [ 'methods' => 'POST', 'callback' => static fn () => 'post', 'permission_callback' => '__return_true' ], true );
    register_rest_route( 'acme/v1', '/overlap', [ 'methods' => 'POST', 'callback' => static fn () => 'A', 'permission_callback' => '__return_true' ] );
    register_rest_route( 'acme/v1', '/overlap', [ 'methods' => 'GET, POST', 'callback' => static fn () => 'B', 'permission_callback' => '__return_true' ] );
    register_rest_route( 'acme/v1', '/truthy', [ 'methods' => 'GET', 'callback' => static fn () => 'ok', 'permission_callback' => static fn () => 1 ] );
    register_rest_route( 'acme/v1', '/hardened', [ 'methods' => 'GET', 'callback' => static fn () => 'open', 'permission_callback' => '__return_true' ] );
    register_rest_route( 'acme/v1', '/perm-false', [ 'methods' => 'GET', 'callback' => static fn () => 'ran', 'permission_callback' => '__return_false' ] );
    register_rest_route( 'acme/v1', '/perm-error', [ 'methods' => 'GET', 'callback' => static fn () => 'ran', 'permission_callback' => static fn () => new WP_Error( 'x', 'X', [ 'status' => 418 ] ) ] );
    register_rest_route( 'acme/v1', '/no-perm', [ 'methods' => 'GET', 'callback' => static fn () => 'open' ] );
    register_rest_route( 'acme/v1', '/args', [
        'methods'             => 'GET',
        'callback'            => static fn ( $request ) => [ 'id' => $request['id'], 'tag' => $request['tag'] ],
        'permission_callback' => $public,
        'args'                => [
            'id'  => [ 'type' => 'integer', 'required' => true ],
            'tag' => [ 'type' => 'string', 'enum' => [ 'a', 'b' ] ],
        ],
    ] );
    register_rest_route( 'acme/v1', '/defaults', [
        'methods'             => 'GET',
        'callback'            => static fn ( $request ) => $request['n'],
        'permission_callback' => $public,
        'args'                => [ 'n' => [ 'default' => 5, 'sanitize_callback' => 'absint' ] ],
    ] );
    register_rest_route( 'acme/v1', '/resp', [
        'methods'             => 'GET',
        'callback'            => static function () {
            $response = new WP_REST_Response( [ 'x' => 1 ], 201, [ 'X-A' => '1' ] );
            $response->add_link( 'about', 'http://x.test/a' );
            return $response;
        },
        'permission_callback' => $public,
    ] );
    register_rest_route( 'acme/v1', '/error', [
        'methods'             => 'GET',
        'callback'            => static fn () => new WP_Error( 'y', 'Y', [ 'status' => 409 ] ),
        'permission_callback' => $public,
    ] );
}
add_action( 'rest_api_init', 'legacy_routes' );
function legacy_harden( $endpoints ) {
    if ( isset( $endpoints['/acme/v1/hardened'][0] ) ) {
        $endpoints['/acme/v1/hardened'][0]['permission_callback'] = '__return_false';
    }
    return $endpoints;
}
add_filter( 'rest_endpoints', 'legacy_harden' );
"#;

fn request(b: &Booted, globals: &str) {
    load_release(&b.php);
    load(&b.php, PLUGIN);
    load(&b.php, globals);
    load(&b.php, "do_action( 'init' );");
}

fn call(b: &Booted, path: &str) -> (u16, Json) {
    let server = b.service.server().expect("a server");
    let request = MessageFactory
        .create_server_request(Method::Get, &format!("http://x.test{path}"), vec![])
        .unwrap();
    let response = server.serve(&request).expect("a REST request");
    let body = response.body().borrow_mut().read_all().unwrap();
    (
        response.status().as_u16(),
        serde_json::from_slice(&body).unwrap_or(Json::Null),
    )
}

fn main() {
    let b = boot();
    request(&b, "");

    // rest_api_init_registers_through_the_shim
    let server = b.service.server().expect("a server");
    let found = server.table().find(RestMethod::Get, "/acme/v1/r/3");
    let operation = match &found {
        RouteMatch::Found { operation, .. } => operation.clone(),
        RouteMatch::NoRoute => String::new(),
    };
    let spec = server.registries().get_operation(&operation);
    let (status, body) = call(&b, "/wp-json/acme/v1/r/3");
    check(
        "rest_api_init_registers_through_the_shim",
        operation.starts_with("legacy-rest/")
            && spec
                .as_ref()
                .is_some_and(|s| s.exposed_on("rest") && !s.exposed_on("xmlrpc"))
            && status == 200
            && body == serde_json::json!({"id": 3, "label": "one"}),
        (&found, status, &body),
    );

    // duplicate_without_override_first_wins, override_replaces
    let dup = call(&b, "/wp-json/acme/v1/dup");
    let over = call(&b, "/wp-json/oembed/1.0/over");
    check(
        "duplicate_without_override_first_wins",
        dup == (200, Json::from("first")),
        &dup,
    );
    check(
        "override_replaces",
        over == (200, Json::from("plugin")),
        &over,
    );

    // get_routes_has_trunk_shape
    let methods = eval(
        &b.php,
        r"return rest_get_server()->get_routes( 'acme/v1' )['/acme/v1/r/(?P<id>\d+)'][0]['methods'] ?? null;",
    );
    check(
        "get_routes_has_trunk_shape",
        matches!(&methods, Ok(PhpValue::Array(items)) if items.len() == 1),
        &methods,
    );

    // permission_false_is_401_or_403
    let anonymous = call(&b, "/wp-json/acme/v1/perm-false");
    load(&b.php, "$GLOBALS['uid'] = 8;");
    let signed_in = call(&b, "/wp-json/acme/v1/perm-false");
    load(&b.php, "$GLOBALS['uid'] = 0;");
    check(
        "permission_false_is_401_or_403",
        anonymous.0 == 401 && anonymous.1["code"] == "rest_forbidden" && signed_in.0 == 403,
        (&anonymous, &signed_in),
    );

    // permission_wp_error_keeps_its_status
    let error = call(&b, "/wp-json/acme/v1/perm-error");
    check(
        "permission_wp_error_keeps_its_status",
        error.0 == 418 && error.1["code"] == "x",
        &error,
    );

    // missing_permission_callback_is_public_with_a_notice
    let open = call(&b, "/wp-json/acme/v1/no-perm");
    let notice = eval(
        &b.php,
        "return implode( '|', $GLOBALS['__doing_it_wrong'] ?? [] );",
    );
    check(
        "missing_permission_callback_is_public_with_a_notice",
        open == (200, Json::from("open"))
            && matches!(&notice, Ok(PhpValue::Str(n)) if n.contains("permission_callback")),
        (&open, &notice),
    );

    // args_validate_in_wordpress_words
    let missing = call(&b, "/wp-json/acme/v1/args");
    let bad_tag = call(&b, "/wp-json/acme/v1/args?id=3&tag=c");
    let good = call(&b, "/wp-json/acme/v1/args?id=3&tag=a");
    let wordpress = eval(
        &b.php,
        "$e = rest_validate_value_from_schema( 'c', [ 'type' => 'string', 'enum' => [ 'a', 'b' ] ], 'tag' ); return $e->get_error_message();",
    );
    check(
        "args_validate_in_wordpress_words",
        missing.0 == 400
            && missing.1["code"] == "rest_missing_callback_param"
            && bad_tag.0 == 400
            && bad_tag.1["code"] == "rest_invalid_param"
            && matches!(&wordpress, Ok(PhpValue::Str(m)) if bad_tag.1["data"]["params"]["tag"] == m.as_str())
            && good == (200, serde_json::json!({"id": 3, "tag": "a"})),
        (&missing, &bad_tag, &wordpress, &good),
    );

    // defaults_and_sanitize_reach_the_callback
    let defaulted = call(&b, "/wp-json/acme/v1/defaults");
    let sanitized = call(&b, "/wp-json/acme/v1/defaults?n=-3");
    check(
        "defaults_and_sanitize_reach_the_callback",
        defaulted == (200, Json::from(5)) && sanitized == (200, Json::from(3)),
        (&defaulted, &sanitized),
    );

    // callback_response_keeps_status_headers_and_links
    let server = b.service.server().expect("a server");
    let http = MessageFactory
        .create_server_request(Method::Get, "http://x.test/wp-json/acme/v1/resp", vec![])
        .unwrap();
    let response = server.serve(&http).expect("a REST request");
    let body: Json = serde_json::from_slice(&response.body().borrow_mut().read_all().unwrap())
        .unwrap_or(Json::Null);
    check(
        "callback_response_keeps_status_headers_and_links",
        response.status().as_u16() == 201
            && response.headers().get("X-A").as_deref() == Some("1")
            && body["x"] == 1
            && body["_links"]["about"][0]["href"] == "http://x.test/a",
        (response.status().as_u16(), &body),
    );

    // callback_wp_error_keeps_its_status
    let failed = call(&b, "/wp-json/acme/v1/error");
    check(
        "callback_wp_error_keeps_its_status",
        failed.0 == 409 && failed.1["code"] == "y",
        &failed,
    );

    // override_replaces_the_whole_route
    let swapped_get = call(&b, "/wp-json/acme/v1/swap");
    check(
        "override_replaces_the_whole_route",
        swapped_get.0 == 404,
        &swapped_get,
    );

    // overlapping_methods_resolve_in_registration_order
    let server = b.service.server().expect("a server");
    let post = MessageFactory
        .create_server_request(
            Method::Post,
            "http://x.test/wp-json/acme/v1/overlap",
            vec![],
        )
        .unwrap();
    let posted = server.serve(&post).expect("a REST request");
    let posted: Json = serde_json::from_slice(&posted.body().borrow_mut().read_all().unwrap())
        .unwrap_or(Json::Null);
    let got = call(&b, "/wp-json/acme/v1/overlap");
    check(
        "overlapping_methods_resolve_in_registration_order",
        posted == "A" && got == (200, Json::from("B")),
        (&posted, &got),
    );

    // a_truthy_permission_result_allows
    let truthy = call(&b, "/wp-json/acme/v1/truthy");
    check(
        "a_truthy_permission_result_allows",
        truthy == (200, Json::from("ok")),
        &truthy,
    );

    // rest_endpoints_changes_to_a_handler_take_effect
    let hardened = call(&b, "/wp-json/acme/v1/hardened");
    check(
        "rest_endpoints_changes_to_a_handler_take_effect",
        hardened.0 == 401,
        &hardened,
    );

    // a_new_server_instance_resets_legacy_routes
    load(
        &b.php,
        "$GLOBALS['only_b'] = true; $GLOBALS['wp_rest_server'] = null; rest_get_server();",
    );
    let gone = call(&b, "/wp-json/acme/v1/perm-false");
    let fresh = call(&b, "/wp-json/acme/v1/b");
    load(
        &b.php,
        "$GLOBALS['only_b'] = false; $GLOBALS['wp_rest_server'] = null; rest_get_server();",
    );
    let back = call(&b, "/wp-json/acme/v1/perm-false");
    check(
        "a_new_server_instance_resets_legacy_routes",
        gone.0 == 404 && fresh == (200, Json::from("b")) && back.0 == 401,
        (&gone, &fresh, &back),
    );

    // re_registration_is_a_no_op_that_rebinds
    b.php.restart_request().unwrap();
    request(&b, "$GLOBALS['label'] = 'two';");
    let registries = b.service.server().expect("a server").registries().clone();
    let (_, second) = call(&b, "/wp-json/acme/v1/r/3");
    check(
        "re_registration_is_a_no_op_that_rebinds",
        second["label"] == "two" && registries.is_closed(),
        &second,
    );
}
