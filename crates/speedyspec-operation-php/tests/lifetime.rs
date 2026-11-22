//! Registries across requests: lazy init, survival, per-request callables, rebuilds.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`). PHP forgets every function,
//! class and global at the end of a request, so each request loads the fixture again.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_operation_contracts::{Caller, OperationCatalog, Value};
use speedyspec_operation_dispatch::{bind_handler, handler_fn};
use speedyspec_operation_php::convert::to_php;
use speedyspec_operation_php::{SiteId, SiteKey};
use speedyspec_php_ext::Value as PhpValue;
use support::*;

const FIXTURE: &str = r#"
use SpeedySpec\Operation\{S, Type, Operation, Operations, Handler, OperationException};

function acme_allow( $caller, $input ): bool { return true; }
function acme_a( array $input, $caller ): array { return [ 'title' => $GLOBALS['title'] ?? 'one' ]; }
function acme_b_php( array $input, $caller ): array { return [ 'title' => 'php' ]; }
function acme_init(): void {
    $GLOBALS['inits'] = ( $GLOBALS['inits'] ?? 0 ) + 1;
    $out = S::object( [ 'title' => S::string()->required() ] );
    Operation::query( 'acme/a' )->input( null, S::object( [] ) )->output( null, $out )->permission( 'acme_allow' )->register();
    Handler::for( 'acme/a', 'acme_a' );
    if ( ! empty( $GLOBALS['throw'] ) ) {
        throw new RuntimeException( 'init broke' );
    }
    if ( empty( $GLOBALS['skip_b'] ) ) {
        Operation::query( 'acme/b' )->input( null, S::object( [] ) )->output( null, $out )->permission( 'acme_allow' )->register();
    }
    if ( ! empty( $GLOBALS['override_b'] ) ) {
        Handler::for( 'acme/b', 'acme_b_php' );
    }
    if ( ! empty( $GLOBALS['broken'] ) ) {
        Type::resource( 'broken' )->field( 'id', S::integer() )->relation( 'ghost', 'acme/ghost', [ 'id' => 'id' ] )->register();
        Operation::query( 'acme/c' )->input( null, S::object( [] ) )->output( null, Type::ref( 'broken' ) )->public()->register();
    }
}
add_action( 'wp_operations_init', 'acme_init' );

function title_of( string $name ): string {
    try {
        return Operations::call( $name, [], 7 )['title'];
    } catch ( OperationException $e ) {
        return 'error:' . $e->getErrorCode();
    } catch ( Throwable $e ) {
        return 'threw:' . $e->getMessage();
    }
}
"#;

struct MovableSiteKey(Rc<RefCell<SiteId>>);

impl SiteKey for MovableSiteKey {
    fn current(&self) -> SiteId {
        self.0.borrow().clone()
    }
}

fn site(fingerprint: &str) -> SiteId {
    SiteId {
        blog: 1,
        fingerprint: fingerprint.into(),
    }
}

fn next_request(b: &Booted, globals: &str) {
    b.php.restart_request().expect("the next request starts");
    load(&b.php, HOOKS);
    load(&b.php, FIXTURE);
    load(&b.php, globals);
    load(&b.php, READY);
}

fn title(b: &Booted, name: &str) -> String {
    match eval(&b.php, &format!("return title_of( '{name}' );")) {
        Ok(PhpValue::Str(s)) => s,
        other => format!("{other:?}"),
    }
}

fn inits(b: &Booted) -> i64 {
    match eval(&b.php, "return $GLOBALS['inits'] ?? 0;") {
        Ok(PhpValue::Int(n)) => n,
        _ => -1,
    }
}

fn main() {
    let key = Rc::new(RefCell::new(site("a")));
    let b = boot(identity(), Rc::new(MovableSiteKey(key.clone())));
    // acme/b's handler is Rust, for the worker: it answers whenever no PHP handler overrides it.
    let rust_b = handler_fn(|_: PhpValue, _: &Caller| {
        Ok(to_php(&Value::map([("title", Value::from("rust"))])))
    });
    bind_handler(&b.container, "acme/b", move |_, _| Ok(rust_b.clone()));

    // not_ready_before_the_plugin_api_or_init
    let no_api = b.area.site().err().map(|e| e.code);
    load(&b.php, HOOKS);
    load(&b.php, FIXTURE);
    let before_init = title(&b, "acme/a");
    check(
        "not_ready_before_the_plugin_api_or_init",
        no_api.as_deref() == Some("registry_not_ready")
            && before_init == "error:registry_not_ready"
            && inits(&b) == 0
            && b.area.sites().site_ids().is_empty(),
        (&no_api, &before_init, inits(&b), b.area.sites().site_ids()),
    );
    load(&b.php, READY);

    // init_is_lazy
    let before = inits(&b);
    let first = title(&b, "acme/a");
    let after_one = inits(&b);
    let again = title(&b, "acme/a");
    check(
        "init_is_lazy",
        before == 0 && first == "one" && after_one == 1 && again == "one" && inits(&b) == 1,
        (before, &first, after_one, &again, inits(&b)),
    );
    let registries = b.area.site().unwrap().registries.clone();

    // registries_survive_requests, second_request_uses_its_own_callables
    next_request(&b, "$GLOBALS['title'] = 'two';");
    let second = title(&b, "acme/a");
    let now = b.area.site().unwrap().registries.clone();
    check(
        "registries_survive_requests",
        Rc::ptr_eq(&registries, &now) && now.is_closed() && inits(&b) == 1,
        (inits(&b), now.is_closed()),
    );
    check(
        "second_request_uses_its_own_callables",
        second == "two",
        &second,
    );

    // missing_rebind_is_reported
    next_request(&b, "$GLOBALS['skip_b'] = true;");
    let b_missing = title(&b, "acme/b");
    let a_fine = title(&b, "acme/a");
    check(
        "missing_rebind_is_reported",
        b_missing == "error:permission_callback_missing" && a_fine == "one",
        (&b_missing, &a_fine),
    );

    // php_override_ends_with_the_request
    next_request(&b, "$GLOBALS['override_b'] = true;");
    let overridden = title(&b, "acme/b");
    next_request(&b, "");
    let restored = title(&b, "acme/b");
    check(
        "php_override_ends_with_the_request",
        overridden == "php" && restored == "rust",
        (&overridden, &restored),
    );

    // broken_relation_drops_only_its_owner, fingerprint_change_rebuilds
    *key.borrow_mut() = site("b");
    next_request(&b, "$GLOBALS['broken'] = true;");
    let a_after = title(&b, "acme/a");
    let c = title(&b, "acme/c");
    let rebuilt = b.area.site().unwrap();
    let problems: Vec<String> = rebuilt.problems().iter().map(|p| p.to_string()).collect();
    check(
        "broken_relation_drops_only_its_owner",
        a_after == "one"
            && c == "error:operation_not_found"
            && problems.iter().any(|p| p.contains("broken"))
            && rebuilt.registries.get_operation("acme/c").is_none()
            && inits(&b) == 1,
        (&a_after, &c, &problems),
    );
    next_request(&b, "$GLOBALS['broken'] = true;");
    let a_again = title(&b, "acme/a");
    let c_again = title(&b, "acme/c");
    check(
        "a_dropped_definition_does_not_break_later_requests",
        a_again == "one" && c_again == "error:operation_not_found",
        (&a_again, &c_again),
    );
    check(
        "fingerprint_change_rebuilds",
        !Rc::ptr_eq(&registries, &rebuilt.registries)
            && b.area.sites().site_ids() == vec![site("b")],
        b.area.sites().site_ids(),
    );

    // fatal_mid_registration_rebuilds
    *key.borrow_mut() = site("c");
    next_request(&b, "$GLOBALS['throw'] = true;");
    let thrown = title(&b, "acme/a");
    next_request(&b, "");
    let recovered = title(&b, "acme/a");
    let healthy = b.area.site().unwrap();
    check(
        "fatal_mid_registration_rebuilds",
        thrown == "error:operations_init_failed"
            && recovered == "one"
            && healthy.registries.is_closed()
            && healthy.problems().is_empty(),
        (&thrown, &recovered, healthy.problems()),
    );

    // init_failure_on_closed_registries_still_serves
    next_request(&b, "$GLOBALS['throw'] = true;");
    let served = title(&b, "acme/a");
    check(
        "init_failure_on_closed_registries_still_serves",
        served == "one",
        &served,
    );
}
