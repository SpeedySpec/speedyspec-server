//! A plugin's operations served over HTTP by the Rust REST server, with WordPress's hooks in PHP.

mod support;

use std::rc::Rc;

use serde_json::{Value as Json, json};
use speedyspec_http::{Message, Method, Response, ServerRequestFactory};
use speedyspec_http_message::MessageFactory;
use speedyspec_operation::{Operation, S};
use speedyspec_operation_contracts::{Caller, Extract, Hydrate, OperationError, Value};
use speedyspec_operation_dispatch::{bind_handler, handler_fn};
use speedyspec_operation_php::SiteId;
use speedyspec_php_ext::Value as PhpValue;
use speedyspec_rest::{RestServer, RestTransport};
use speedyspec_rest_php::{PhpRestAuthenticator, PhpRestHooks};
use speedyspec_transport::TransportRegistry;
use support::*;

const PLUGIN: &str = r#"
use SpeedySpec\Operation\{S, Type, Operation, Handler, Caller, Proxy};
use SpeedySpec\Rest\Rest;

final class Report {
    public function __construct( public readonly int $id, public readonly string $title, public readonly int $author_id ) {}
}
final class GetReport {
    public function __construct( public readonly int $id, public readonly ?string $title = null ) {}
}
function acme_can_read( Caller $caller, GetReport $input ): bool { return $caller->can( 'edit_report' ); }
function acme_get_report( GetReport $input, Caller $caller ): Report {
    $GLOBALS['handled'] = ( $GLOBALS['handled'] ?? 0 ) + 1;
    return new Report( $input->id, $input->title ?? 'Q3', 9 );
}
function acme_create_report( array $input, Caller $caller ): array {
    return [ 'id' => 4, 'title' => $input['title'] ?? 'new', 'author_id' => 9 ];
}
function acme_list_reports( array $input, Caller $caller ): array {
    $GLOBALS['list_id'] = $input['id'] ?? null;
    return [];
}
function core_get_user( array $input, Caller $caller ): array { return [ 'id' => $input['id'] ]; }
function rust_ping( Proxy $input, Caller $caller ): array { return [ 'greeting' => 'php ' . $input->name ]; }
function acme_init(): void {
    Type::resource( 'report' )
        ->field( 'id', S::integer()->required() )
        ->field( 'title', S::string()->required() )
        ->field( 'author_id', S::integer() )
        ->relation( 'author', 'core/get-user', [ 'id' => 'author_id' ] )
        ->register();
    Type::resource( 'user' )->field( 'id', S::integer()->required() )->register();
    Operation::query( 'acme/get-report' )
        ->resource( 'report', 'get', 'id' )
        ->input( GetReport::class, S::object( [ 'id' => S::integer()->required(), 'title' => S::string() ] ) )
        ->output( Report::class, Type::ref( 'report' ) )
        ->permission( 'acme_can_read' )
        ->register();
    Handler::for( 'acme/get-report', 'acme_get_report' );
    Operation::command( 'acme/create-report' )
        ->resource( 'report', 'create' )
        ->input( null, S::object( [ 'title' => S::string() ] ) )
        ->output( null, Type::ref( 'report' ) )
        ->public()
        ->register();
    Handler::for( 'acme/create-report', 'acme_create_report' );
    Operation::query( 'acme/list-reports' )
        ->resource( 'report', 'list' )
        ->input( null, S::object( [ 'id' => S::integer() ] ) )
        ->output( null, S::array( Type::ref( 'report' ) ) )
        ->public()
        ->register();
    Handler::for( 'acme/list-reports', 'acme_list_reports' );
    Operation::query( 'core/get-user' )
        ->resource( 'user', 'get', 'id' )
        ->input( null, S::object( [ 'id' => S::integer()->required() ] ) )
        ->output( null, Type::ref( 'user' ) )
        ->public()
        ->register();
    Handler::for( 'core/get-user', 'core_get_user' );
    Handler::for( 'rust/ping', 'rust_ping' );
    if ( ! empty( $GLOBALS['rebind'] ) ) {
        Rest::bind( 'acme/get-report' )->route( 'acme/v2', '/report/(?P<id>\d+)' )->methods( 'GET' );
    }
}
add_action( 'wp_operations_init', 'acme_init' );
function acme_block( $result, $server, $request ) {
    if ( ! empty( $GLOBALS['block'] ) && str_contains( $request->get_route(), '/reports/' ) ) {
        return new WP_Error( 'acme_blocked', 'Blocked.', [ 'status' => 403 ] );
    }
    return $result;
}
add_filter( 'rest_pre_dispatch', 'acme_block', 10, 3 );
function acme_cap( $response, $handler, $request ) {
    if ( isset( $GLOBALS['cap'] ) ) {
        $request->set_param( $GLOBALS['cap_key'] ?? 'title', $GLOBALS['cap'] );
    }
    return $response;
}
add_filter( 'rest_request_before_callbacks', 'acme_cap', 10, 3 );
function get_current_user_id() { return $GLOBALS['uid'] ?? 0; }
"#;

struct Ping(String);

impl Hydrate for Ping {
    fn hydrate(value: &Value) -> Result<Self, OperationError> {
        Ok(Ping(match value.get("name") {
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        }))
    }
}

struct Pong(String);

impl Extract for Pong {
    fn extract(&self) -> Value {
        Value::map([("greeting", Value::from(self.0.as_str()))])
    }
}

fn server(b: &Booted) -> RestServer {
    let site = b.operations.site().expect("the site is ready");
    let transports = TransportRegistry::new(b.bindings.clone());
    transports
        .install(Rc::new(RestTransport::new(site.registries.clone())))
        .unwrap();
    let exposure = Rc::new(transports.expose(&site.registries).unwrap());
    RestServer::new(
        b.container.clone(),
        site.registries.clone(),
        exposure,
        Rc::new(PhpRestHooks::new()),
        Rc::new(PhpRestAuthenticator::new(b.identity.clone())),
        "http://x.test/wp-json",
    )
}

fn call(server: &RestServer, method: Method, path: &str) -> (u16, Json) {
    let request = MessageFactory
        .create_server_request(method, &format!("http://x.test{path}"), vec![])
        .unwrap();
    let response = server.serve(&request).expect("a REST request");
    let body = response.body().borrow_mut().read_all().unwrap();
    (
        response.status().as_u16(),
        serde_json::from_slice(&body).unwrap_or(Json::Null),
    )
}

fn set(b: &Booted, code: &str) {
    load(&b.php, code);
}

fn global(b: &Booted, name: &str) -> PhpValue {
    eval(&b.php, &format!("return $GLOBALS['{name}'] ?? null;")).unwrap_or(PhpValue::Null)
}

fn load_request(b: &Booted, globals: &str) {
    load_wordpress(&b.php);
    load(&b.php, PLUGIN);
    load(&b.php, globals);
    load(&b.php, "do_action( 'init' );");
}

fn main() {
    let b = boot();
    // A Rust-contract operation, registered before the first request needs operations; its Rust handler answers unless
    // PHP replaces it.
    b.operations
        .sites()
        .registering(&SiteId {
            blog: 1,
            fingerprint: String::new(),
        })
        .registries
        .register_operation(
            Operation::query("rust/ping")
                .input::<Ping>(S::object([("name", S::string())]))
                .output::<Pong>(S::object([("greeting", S::string().required())]))
                .public()
                .build()
                .unwrap(),
        )
        .unwrap();
    let rust = handler_fn(|p: Ping, _: &Caller| Ok(Pong(format!("rust {}", p.0))));
    bind_handler(&b.container, "rust/ping", move |_, _| Ok(rust.clone()));
    load_request(&b, "");
    let s = server(&b);
    assert!(s.problems().is_empty(), "{:?}", s.problems());

    // get_over_http
    set(&b, "$GLOBALS['uid'] = 7;");
    let (status, body) = call(&s, Method::Get, "/wp-json/acme/v1/reports/3");
    check(
        "get_over_http",
        status == 200
            && body["id"] == 3
            && body["title"] == "Q3"
            && body["_links"]["author"][0]["href"] == "http://x.test/wp-json/core/v1/users/9",
        (status, &body),
    );

    // embed_over_http
    let (_, embedded) = call(&s, Method::Get, "/wp-json/acme/v1/reports/3?_embed=1");
    check(
        "embed_over_http",
        embedded["_embedded"]["author"][0]["id"] == 9,
        &embedded,
    );

    // security_plugin_wp_error_is_its_403
    let before = global(&b, "handled");
    set(&b, "$GLOBALS['block'] = true;");
    let (status, blocked) = call(&s, Method::Get, "/wp-json/acme/v1/reports/3");
    set(&b, "$GLOBALS['block'] = false;");
    check(
        "security_plugin_wp_error_is_its_403",
        status == 403
            && blocked["code"] == "acme_blocked"
            && blocked["message"] == "Blocked."
            && global(&b, "handled") == before,
        (status, &blocked, before, global(&b, "handled")),
    );

    // before_callbacks_set_param_reaches_handler
    set(&b, "$GLOBALS['cap'] = 'capped';");
    let (_, capped) = call(&s, Method::Get, "/wp-json/acme/v1/reports/3");
    set(&b, "$GLOBALS['cap'] = 'x'; $GLOBALS['cap_key'] = 'id';");
    let (status, invalid) = call(&s, Method::Get, "/wp-json/acme/v1/reports/3");
    set(&b, "unset( $GLOBALS['cap'], $GLOBALS['cap_key'] );");
    check(
        "before_callbacks_set_param_reaches_handler",
        capped["title"] == "capped" && status == 400 && invalid["code"] == "rest_invalid_param",
        (&capped, status, &invalid),
    );

    // anonymous_is_401_signed_in_is_403
    set(&b, "$GLOBALS['uid'] = 0;");
    let anonymous = call(&s, Method::Get, "/wp-json/acme/v1/reports/3");
    set(&b, "$GLOBALS['uid'] = 8;");
    let other = call(&s, Method::Get, "/wp-json/acme/v1/reports/3");
    check(
        "anonymous_is_401_signed_in_is_403",
        anonymous.0 == 401 && anonymous.1["code"] == "rest_forbidden" && other.0 == 403,
        (&anonymous, &other),
    );

    // create_is_201, query_string_coerces
    let (created, made) = call(&s, Method::Post, "/wp-json/acme/v1/reports");
    let (listed, _) = call(&s, Method::Get, "/wp-json/acme/v1/reports?id=3");
    check(
        "create_is_201_and_query_string_coerces",
        created == 201
            && made["id"] == 4
            && listed == 200
            && global(&b, "list_id") == PhpValue::Int(3),
        (created, &made, listed, global(&b, "list_id")),
    );

    // php_handler_replaces_rust_handler_over_rest
    let (status, ping) = call(&s, Method::Get, "/wp-json/rust/v1/ping?name=ada");
    check(
        "php_handler_replaces_rust_handler_over_rest",
        status == 200 && ping == json!({"greeting": "php ada"}),
        (status, &ping),
    );

    // rest_bind_overrides_the_default_route
    b.php.restart_request().expect("the next request starts");
    load_request(&b, "$GLOBALS['rebind'] = true; $GLOBALS['uid'] = 7;");
    let rebound = server(&b);
    let v2 = call(&rebound, Method::Get, "/wp-json/acme/v2/report/3");
    let v1 = call(&rebound, Method::Get, "/wp-json/acme/v1/reports/3");
    check(
        "rest_bind_overrides_the_default_route",
        v2.0 == 200 && v2.1["id"] == 3 && v1.0 == 404,
        (&v2, &v1, rebound.problems()),
    );
}
