//! RestService: one REST server per site, rebuilt when the site or its bindings change.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_operation_php::{SiteId, SiteKey};
use speedyspec_rest::RouteMatch;
use speedyspec_rest_contracts::RestMethod;
use support::*;

const PLUGIN: &str = r#"
use SpeedySpec\Operation\{S, Operation, Handler};
use SpeedySpec\Rest\Rest;
function svc_ping( array $input, $caller ): array { return []; }
function svc_init(): void {
    Operation::query( 'svc/ping' )->input( null, S::object( [] ) )->output( null, S::object( [] ) )->public()->register();
    Handler::for( 'svc/ping', 'svc_ping' );
    if ( ! empty( $GLOBALS['rebind'] ) ) {
        Rest::bind( 'svc/ping' )->route( 'svc/v2', '/pong' )->methods( 'GET' );
    }
}
add_action( 'wp_operations_init', 'svc_init' );
"#;

struct Movable(Rc<RefCell<SiteId>>);

impl SiteKey for Movable {
    fn current(&self) -> SiteId {
        self.0.borrow().clone()
    }
}

fn request(b: &Booted, globals: &str) {
    load_wordpress(&b.php);
    load(&b.php, PLUGIN);
    load(&b.php, globals);
    load(&b.php, "do_action( 'init' );");
}

fn main() {
    let key = Rc::new(RefCell::new(SiteId {
        blog: 1,
        fingerprint: "a".into(),
    }));
    let b = boot_with_key(Rc::new(Movable(key.clone())));
    request(&b, "");

    // server_is_cached_per_site
    let first = b.service.server().expect("a server");
    let again = b.service.server().expect("a server");
    check("server_is_cached_per_site", Rc::ptr_eq(&first, &again), ());

    // rebinding_rebuilds_the_server
    b.php.restart_request().unwrap();
    request(&b, "$GLOBALS['rebind'] = true;");
    let rebound = b.service.server().expect("a server");
    let found = rebound.table().find(RestMethod::Get, "/svc/v2/pong");
    check(
        "rebinding_rebuilds_the_server",
        !Rc::ptr_eq(&first, &rebound)
            && matches!(&found, RouteMatch::Found { operation, .. } if operation == "svc/ping"),
        &found,
    );

    // a_new_site_gets_a_new_server
    *key.borrow_mut() = SiteId {
        blog: 1,
        fingerprint: "b".into(),
    };
    b.php.restart_request().unwrap();
    request(&b, "$GLOBALS['rebind'] = true;");
    let other = b.service.server().expect("a server");
    check(
        "a_new_site_gets_a_new_server",
        !Rc::ptr_eq(&rebound, &other),
        (),
    );
}
