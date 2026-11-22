//! WP_REST_Server::dispatch and rest_do_request reach every operation: the LegacyCallerAdapter.

mod support;

use speedyspec_php_ext::Value as PhpValue;
use support::*;

const PLUGIN: &str = r#"
use SpeedySpec\Operation\{S, Type, Operation, Handler, Caller};

final class CallerReport {
    public function __construct( public readonly int $id, public readonly string $title, public readonly int $author_id ) {}
}
function caller_get_report( array $input, Caller $caller ): CallerReport { return new CallerReport( $input['id'], 'Q3', 9 ); }
function caller_list_reports( array $input, Caller $caller ): array { $GLOBALS['list_id'] = $input['id'] ?? null; return []; }
function caller_get_user( array $input, Caller $caller ): array { return [ 'id' => $input['id'] ]; }
function caller_init(): void {
    Type::resource( 'report' )
        ->field( 'id', S::integer()->required() )
        ->field( 'title', S::string()->required() )
        ->field( 'author_id', S::integer() )
        ->relation( 'author', 'core/get-user', [ 'id' => 'author_id' ] )
        ->register();
    Type::resource( 'user' )->field( 'id', S::integer()->required() )->register();
    Operation::query( 'acme/get-report' )->resource( 'report', 'get', 'id' )
        ->input( null, S::object( [ 'id' => S::integer()->required() ] ) )
        ->output( CallerReport::class, Type::ref( 'report' ) )->public()->register();
    Handler::for( 'acme/get-report', 'caller_get_report' );
    Operation::query( 'acme/list-reports' )->resource( 'report', 'list' )
        ->input( null, S::object( [ 'id' => S::integer() ] ) )
        ->output( null, S::array( Type::ref( 'report' ) ) )->public()->register();
    Handler::for( 'acme/list-reports', 'caller_list_reports' );
    Operation::query( 'core/get-user' )->resource( 'user', 'get', 'id' )
        ->input( null, S::object( [ 'id' => S::integer()->required() ] ) )
        ->output( null, Type::ref( 'user' ) )->public()->register();
    Handler::for( 'core/get-user', 'caller_get_user' );
}
add_action( 'wp_operations_init', 'caller_init' );
function caller_legacy( $server ) {
    register_rest_route( 'acme-legacy/v1', '/thing', [ 'methods' => 'GET', 'callback' => static fn () => 'thing', 'permission_callback' => '__return_true' ] );
}
add_action( 'rest_api_init', 'caller_legacy' );
function caller_count_post_dispatch( $response ) { $GLOBALS['post_dispatch'] = ( $GLOBALS['post_dispatch'] ?? 0 ) + 1; return $response; }
add_filter( 'rest_post_dispatch', 'caller_count_post_dispatch' );
"#;

fn run(b: &Booted, code: &str) -> PhpValue {
    eval(&b.php, code).unwrap_or_else(|e| PhpValue::Str(format!("{e:?}")))
}

fn text(v: &PhpValue) -> String {
    format!("{v:?}")
}

fn main() {
    let b = boot();
    load_release(&b.php);
    load(&b.php, PLUGIN);
    load(&b.php, "do_action( 'init' );");

    // rest_do_request_reaches_a_new_style_operation
    let report = run(
        &b,
        r"$r = rest_do_request( new WP_REST_Request( 'GET', '/acme/v1/reports/3' ) );
          return json_encode( [ $r->get_status(), $r->get_data()['title'] ?? null, isset( $r->get_links()['author'] ), $r->get_matched_route() ] );",
    );
    check(
        "rest_do_request_reaches_a_new_style_operation",
        report == PhpValue::Str(r#"[200,"Q3",true,"\/acme\/v1\/reports\/(?P<id>[\\d]+)"]"#.into()),
        &report,
    );

    // dispatch_data_has_no_links
    let raw = run(
        &b,
        "$r = rest_do_request( new WP_REST_Request( 'GET', '/acme/v1/reports/3' ) ); return json_encode( [ array_key_exists( '_links', $r->get_data() ), isset( $r->get_links()['author'] ) ] );",
    );
    check(
        "dispatch_data_has_no_links",
        raw == PhpValue::Str("[false,true]".into()),
        &raw,
    );

    // an_unknown_method_is_no_route
    let purge = run(
        &b,
        "$r = rest_do_request( new WP_REST_Request( 'PURGE', '/acme/v1/reports/3' ) ); return $r->get_status();",
    );
    check(
        "an_unknown_method_is_no_route",
        purge == PhpValue::Int(404),
        &purge,
    );

    // rest_do_request_reaches_a_legacy_route
    let legacy = run(
        &b,
        "$r = rest_do_request( '/acme-legacy/v1/thing' ); return json_encode( [ $r->get_status(), $r->get_data() ] );",
    );
    check(
        "rest_do_request_reaches_a_legacy_route",
        legacy == PhpValue::Str(r#"[200,"thing"]"#.into()),
        &legacy,
    );

    // unknown_route_is_a_404_response
    let unknown = run(
        &b,
        "$r = rest_do_request( '/nope/v1/nothing' ); return json_encode( [ $r->get_status(), $r->get_data()['code'] ?? null ] );",
    );
    check(
        "unknown_route_is_a_404_response",
        unknown == PhpValue::Str(r#"[404,"rest_no_route"]"#.into()),
        &unknown,
    );

    // query_params_set_on_the_request_arrive
    let listed = run(
        &b,
        "$q = new WP_REST_Request( 'GET', '/acme/v1/reports' ); $q->set_query_params( [ 'id' => '3' ] ); rest_do_request( $q ); return $GLOBALS['list_id'];",
    );
    check(
        "query_params_set_on_the_request_arrive",
        listed == PhpValue::Int(3),
        &listed,
    );

    // neither_dispatch_nor_rest_do_request_fires_post_dispatch
    let count = run(&b, "return $GLOBALS['post_dispatch'] ?? 0;");
    check(
        "neither_dispatch_nor_rest_do_request_fires_post_dispatch",
        count == PhpValue::Int(0),
        &count,
    );

    // the_server_lists_both_kinds
    let listed = run(
        &b,
        r"$s = rest_get_server(); $n = $s->get_namespaces(); $routes = $s->get_routes();
          return json_encode( [ in_array( 'acme/v1', $n, true ), in_array( 'acme-legacy/v1', $n, true ), isset( $routes['/acme/v1/reports/(?P<id>[\d]+)'] ), isset( $routes['/acme-legacy/v1/thing'] ) ] );",
    );
    check(
        "the_server_lists_both_kinds",
        listed == PhpValue::Str("[true,true,true,true]".into()),
        text(&listed),
    );
}
