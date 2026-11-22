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
}
add_action( 'rest_api_init', 'legacy_routes' );
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
