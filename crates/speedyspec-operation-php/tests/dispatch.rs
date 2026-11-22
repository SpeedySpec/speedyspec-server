//! PHP handlers and permission callbacks, and calls from PHP.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`).

mod support;

use std::rc::Rc;

use speedyspec_operation::{Operation, S};
use speedyspec_operation_contracts::{
    Caller, Category, Extract, Hydrate, INTERNAL_TRANSPORT, OperationError, Value,
};
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
function test_ping( $input, Caller $caller ) {
    return [];
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
    Handler::for( 'test/ping', 'test_ping' );
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

struct Ping;

impl Hydrate for Ping {
    fn hydrate(_: &Value) -> Result<Self, OperationError> {
        Ok(Ping)
    }
}

impl Extract for Ping {
    fn extract(&self) -> Value {
        Value::map::<&str>([])
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
    // A Rust-contract operation, registered before the first request needs operations.
    b.area
        .sites()
        .registering(&site)
        .registries
        .register_operation(
            Operation::query("test/ping")
                .input::<Ping>(S::object([]))
                .output::<Ping>(S::object([]))
                .public()
                .build()
                .unwrap(),
        )
        .unwrap();

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

    // rust_input_with_php_handler_is_refused
    let ping = error_of(&eval(&b.php, "return call_error( 'test/ping' );"));
    check(
        "rust_input_with_php_handler_is_refused",
        ping.as_ref()
            .is_some_and(|e| e.0 == "handler_contract_mismatch" && e.1 == "internal"),
        &ping,
    );

    let _ = Category::Internal;
}
