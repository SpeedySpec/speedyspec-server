//! PHP handlers and permission callbacks, and calls from PHP.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`).

mod support;

use std::rc::Rc;

use speedyspec_operation::{Operation, S};
use speedyspec_operation_contracts::{
    Caller, Category, Extract, Hydrate, INTERNAL_TRANSPORT, OperationError, Value,
};
use speedyspec_operation_dispatch::{bind_handler, handler_fn};
use speedyspec_operation_php::{FixedSiteKey, SiteId};
use speedyspec_php_ext::Value as PhpValue;
use support::*;

const FIXTURE: &str = r#"
use SpeedySpec\Operation\{S, Type, Operation, Operations, Category, Caller, Handler, OperationException};

final class UpdateReportInput {
    public function __construct(
        public readonly int $id,
        public readonly ?string $title = null,
        public readonly ?DateTimeImmutable $due = null,
    ) {}
}
final class Report {
    public function __construct(
        public readonly int $id,
        public readonly string $title,
        public readonly DateTimeImmutable $due,
        public readonly string $file,
    ) {}
}
function acme_can_edit( Caller $caller, UpdateReportInput $input ): bool {
    return $caller->can( 'edit_report', $input->id );
}
function acme_update_report( UpdateReportInput $input, Caller $caller ): Report {
    if ( ! $input->due instanceof DateTimeImmutable ) {
        throw new LogicException( 'due is not a date' );
    }
    return new Report( $input->id, 'Q3', new DateTimeImmutable( '2026-11-22T10:00:00Z' ), "\xff\x00" );
}
function acme_locked( array $input, Caller $caller ): array {
    throw new OperationException( 'report_locked', Category::Conflict, 'Locked.', [ 'by' => 2 ] );
}
function acme_crash( array $input, Caller $caller ): array {
    throw new RuntimeException( 'secret path /etc' );
}
function acme_meta( array $input, Caller $caller ): array {
    return [ 'meta' => [] ];
}
final class NullableIn {
    public function __construct( public readonly int $id, public readonly ?string $note ) {}
}
function acme_object_permission( Caller $caller, $input ) { return new stdClass(); }
function acme_object_can( Caller $caller, $input ): bool { return $caller->can( 'edit_report', new stdClass() ); }
function acme_echo( NullableIn $input, Caller $caller ): array { return [ 'note' => $input->note ?? 'none' ]; }
function test_ping( \SpeedySpec\Operation\Proxy $input, Caller $caller ): array {
    if ( ! empty( $GLOBALS['bad_ping'] ) ) {
        return [ 'greeting' => 5 ];
    }
    return [ 'greeting' => 'hi ' . $input->name ];
}
function acme_register(): void {
    Type::resource( 'report' )
        ->field( 'id', S::integer()->required() )
        ->field( 'title', S::string()->required() )
        ->field( 'due', S::dateTime()->required() )
        ->field( 'file', S::binary( 'application/pdf' )->required() )
        ->register();
    Operation::command( 'acme/update-report' )
        ->input( UpdateReportInput::class, S::object( [
            'id'    => S::integer()->required(),
            'title' => S::string(),
            'due'   => S::dateTime(),
        ] ) )
        ->output( Report::class, Type::ref( 'report' ) )
        ->permission( 'acme_can_edit' )
        ->register();
    Handler::for( 'acme/update-report', 'acme_update_report' );
    Operation::command( 'acme/locked' )->input( null, S::object( [] ) )->output( null, S::object( [] ) )
        ->error( 'report_locked', Category::Conflict )->public()->register();
    Handler::for( 'acme/locked', 'acme_locked' );
    Operation::command( 'acme/crash' )->input( null, S::object( [] ) )->output( null, S::object( [] ) )->public()->register();
    Handler::for( 'acme/crash', 'acme_crash' );
    Operation::query( 'acme/meta' )->input( null, S::object( [] ) )
        ->output( null, S::object( [ 'meta' => S::object( [] )->required() ] ) )->public()->register();
    Handler::for( 'acme/meta', 'acme_meta' );
    if ( empty( $GLOBALS['no_php_ping'] ) ) {
        Handler::for( 'test/ping', 'test_ping' );
    }
    foreach ( [ 'acme/object-permission' => 'acme_object_permission', 'acme/object-can' => 'acme_object_can' ] as $name => $check ) {
        Operation::query( $name )->input( null, S::object( [] ) )->output( null, S::object( [] ) )->permission( $check )->register();
        Handler::for( $name, 'acme_meta' );
    }
    Operation::query( 'acme/nullable' )
        ->input( NullableIn::class, S::object( [ 'id' => S::integer()->required(), 'note' => S::string()->nullable()->required() ] ) )
        ->output( null, S::object( [ 'note' => S::string()->required() ] ) )->public()->register();
    Handler::for( 'acme/nullable', 'acme_echo' );
}
add_action( 'wp_operations_init', 'acme_register' );

function call_error( string $name, array|object $input = [], ?int $user = null ): array {
    try {
        Operations::call( $name, $input, $user );
        return [ 'ran' ];
    } catch ( OperationException $e ) {
        return [ $e->getErrorCode(), $e->getCategory()->value, $e->getMessage(), $e->getDetails() ];
    }
}
"#;

struct Ping(String);

impl Hydrate for Ping {
    fn hydrate(value: &Value) -> Result<Self, OperationError> {
        match value.get("name") {
            Some(Value::String(name)) => Ok(Ping(name.clone())),
            _ => Ok(Ping(String::new())),
        }
    }
}

struct Pong(String);

impl Extract for Pong {
    fn extract(&self) -> Value {
        Value::map([("greeting", Value::from(self.0.as_str()))])
    }
}

fn error_of(
    result: &Result<PhpValue, speedyspec_php::PhpError>,
) -> Option<(String, String, String, Value)> {
    let value = neutral(result.as_ref().ok()?);
    let Value::List(items) = value else {
        return None;
    };
    match items.as_slice() {
        [
            Value::String(code),
            Value::String(category),
            Value::String(message),
            details,
        ] => Some((
            code.clone(),
            category.clone(),
            message.clone(),
            details.clone(),
        )),
        _ => None,
    }
}

fn main() {
    let site = SiteId {
        blog: 1,
        fingerprint: String::new(),
    };
    let b = boot(identity(), Rc::new(FixedSiteKey(site.clone())));
    load(&b.php, HOOKS);
    load(&b.php, FIXTURE);
    load(&b.php, READY);
    // A Rust-contract operation, registered before the first request needs operations.
    b.area
        .sites()
        .registering(&site)
        .registries
        .register_operation(
            Operation::query("test/ping")
                .input::<Ping>(S::object([("name", S::string().required())]))
                .output::<Pong>(S::object([("greeting", S::string().required())]))
                .public()
                .build()
                .unwrap(),
        )
        .unwrap();
    let rust_ping = handler_fn(|ping: Ping, _: &Caller| Ok(Pong(format!("hello {}", ping.0))));
    bind_handler(&b.container, "test/ping", move |_, _| Ok(rust_ping.clone()));

    let expected =
        r#"[ 'id' => 3, 'title' => 'Q3', 'due' => '2026-11-22T10:00:00', 'file' => "\xff\x00" ]"#;

    // php_call_reaches_php_handler_with_value_objects
    let called = eval(
        &b.php,
        &format!(
            "return {expected} === \\SpeedySpec\\Operation\\Operations::call( 'acme/update-report', [ 'id' => 3, 'due' => '2026-11-22T10:00:00' ], 7 );"
        ),
    );
    check(
        "php_call_reaches_php_handler_with_value_objects",
        matches!(called, Ok(PhpValue::Bool(true))),
        (
            &called,
            eval(
                &b.php,
                "return call_error( 'acme/update-report', [ 'id' => 3, 'due' => '2026-11-22T10:00:00' ], 7 );",
            ),
        ),
    );

    // object_input_is_accepted
    let object = eval(
        &b.php,
        &format!(
            "return {expected} === \\SpeedySpec\\Operation\\Operations::call( 'acme/update-report', new UpdateReportInput( 3, null, new DateTimeImmutable( '2026-11-22T10:00:00Z' ) ), 7 );"
        ),
    );
    check(
        "object_input_is_accepted",
        matches!(object, Ok(PhpValue::Bool(true))),
        &object,
    );

    // permission_denied_for_other_user
    let other = error_of(&eval(
        &b.php,
        "return call_error( 'acme/update-report', [ 'id' => 3 ], 8 );",
    ));
    let anonymous = error_of(&eval(
        &b.php,
        "return call_error( 'acme/update-report', [ 'id' => 3 ], 0 );",
    ));
    check(
        "permission_denied_for_other_user",
        other.as_ref().is_some_and(|e| e.1 == "permission_denied")
            && anonymous.as_ref().is_some_and(|e| e.1 == "unauthenticated"),
        (&other, &anonymous),
    );

    // invalid_input_is_reported
    let invalid = error_of(&eval(
        &b.php,
        "return call_error( 'acme/update-report', [ 'id' => 'x' ], 7 );",
    ));
    check(
        "invalid_input_is_reported",
        invalid.as_ref().is_some_and(|e| {
            e.0 == "invalid_input"
                && e.1 == "invalid_argument"
                && matches!(&e.3, Value::List(l) if !l.is_empty())
        }),
        &invalid,
    );

    // operation_exception_keeps_its_fields
    let locked = error_of(&eval(&b.php, "return call_error( 'acme/locked' );"));
    check(
        "operation_exception_keeps_its_fields",
        locked
            == Some((
                "report_locked".into(),
                "conflict".into(),
                "Locked.".into(),
                Value::map([("by", Value::Int(2))]),
            )),
        &locked,
    );

    // other_throwables_are_generic
    let crash = error_of(&eval(&b.php, "return call_error( 'acme/crash' );"));
    check(
        "other_throwables_are_generic",
        crash.as_ref().is_some_and(|e| {
            e.0 == "handler_failed"
                && e.1 == "internal"
                && e.2 == "The operation failed."
                && !e.2.contains("secret")
        }),
        &crash,
    );

    // empty_array_output_under_object_schema
    let meta_php = eval(
        &b.php,
        "return \\SpeedySpec\\Operation\\Operations::call( 'acme/meta' );",
    );
    let site_ready = b.area.site().expect("the site is ready");
    let meta_rust = site_ready.dispatcher.call(
        "acme/meta",
        Value::map::<&str>([]),
        &Caller::new(None, INTERNAL_TRANSPORT, identity()),
    );
    check(
        "empty_array_output_under_object_schema",
        meta_php
            .as_ref()
            .is_ok_and(|v| neutral(v) == Value::map([("meta", Value::List(vec![]))]))
            && meta_rust == Ok(Value::map([("meta", Value::map::<&str>([]))])),
        (&meta_php, &meta_rust),
    );

    // php_handler_replaces_a_rust_contract_handler
    let replaced = eval(
        &b.php,
        "return \\SpeedySpec\\Operation\\Operations::call( 'test/ping', [ 'name' => 'ada' ] );",
    );
    let bad = error_of(&eval(
        &b.php,
        "$GLOBALS['bad_ping'] = true; $r = call_error( 'test/ping', [ 'name' => 'ada' ] ); $GLOBALS['bad_ping'] = false; return $r;",
    ));
    b.php.restart_request().expect("the next request starts");
    load(&b.php, HOOKS);
    load(&b.php, FIXTURE);
    load(&b.php, "$GLOBALS['no_php_ping'] = true;");
    load(&b.php, READY);
    let rust_again = eval(
        &b.php,
        "return \\SpeedySpec\\Operation\\Operations::call( 'test/ping', [ 'name' => 'ada' ] );",
    );
    check(
        "php_handler_replaces_a_rust_contract_handler",
        replaced
            .as_ref()
            .is_ok_and(|v| neutral(v) == Value::map([("greeting", Value::from("hi ada"))]))
            && bad.as_ref().is_some_and(|e| e.0 == "invalid_output")
            && rust_again
                .as_ref()
                .is_ok_and(|v| neutral(v) == Value::map([("greeting", Value::from("hello ada"))])),
        (&replaced, &bad, &rust_again),
    );

    // a_permission_callback_must_return_true
    let object_permission = error_of(&eval(
        &b.php,
        "return call_error( 'acme/object-permission', [], 7 );",
    ));
    check(
        "a_permission_callback_must_return_true",
        object_permission
            .as_ref()
            .is_some_and(|e| e.1 == "permission_denied"),
        &object_permission,
    );

    // an_object_in_can_denies_without_failing
    let object_can = error_of(&eval(
        &b.php,
        "return call_error( 'acme/object-can', [], 7 );",
    ));
    check(
        "an_object_in_can_denies_without_failing",
        object_can
            .as_ref()
            .is_some_and(|e| e.1 == "permission_denied"),
        &object_can,
    );

    // required_nullable_null_is_kept
    let nullable = eval(
        &b.php,
        "return \\SpeedySpec\\Operation\\Operations::call( 'acme/nullable', new NullableIn( 3, null ) );",
    );
    let optional_null = eval(
        &b.php,
        "return call_error( 'acme/update-report', [ 'id' => 3, 'title' => null, 'due' => '2026-11-22T10:00:00' ], 7 );",
    );
    check(
        "required_nullable_null_is_kept",
        nullable
            .as_ref()
            .is_ok_and(|v| neutral(v) == Value::map([("note", Value::from("none"))]))
            && matches!(&optional_null, Ok(v) if neutral(v) == Value::List(vec![Value::from("ran")])),
        (&nullable, &optional_null),
    );

    let _ = Category::Internal;
}
