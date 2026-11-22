//! Types and operations registered from PHP, into the site's registries.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`).

mod support;

use std::rc::Rc;

use speedyspec_operation::{S, export::json_schema};
use speedyspec_operation_contracts::{OperationCatalog, SchemaKind, Value};
use speedyspec_operation_php::{FixedSiteKey, SiteId};
use speedyspec_php_ext::Value as PhpValue;
use support::*;

const FIXTURE: &str = r#"
use SpeedySpec\Operation\{S, Type, Operation, Category};

final class UpdateReportInput {
    public function __construct( public readonly int $id, public readonly ?string $title = null, public readonly ?string $file = null ) {}
}
final class Report {
    public function __construct( public readonly int $id, public readonly string $title ) {}
}
final class BadInput {
    public function __construct( public readonly string $id ) {}
}
function acme_can_edit( $caller, $input ): bool { return true; }
function acme_register(): void {
    Type::resource( 'report' )->field( 'id', S::integer()->required() )->field( 'title', S::string() )->register();
    Operation::command( 'acme/update-report' )
        ->resource( 'report', 'update', 'id' )
        ->idempotent()
        ->input( UpdateReportInput::class, S::object( [
            'id'    => S::integer()->required(),
            'title' => S::string()->maxLength( 200 ),
            'file'  => S::binary( 'application/pdf' ),
        ] ) )
        ->output( Report::class, Type::ref( 'report' ) )
        ->error( 'report_not_found', Category::NotFound )
        ->error( 'report_locked', Category::Conflict )
        ->permission( 'acme_can_edit' )
        ->label( 'Update report', 'Updates a report and its file.' )
        ->transports( 'rest', 'xmlrpc' )
        ->register();
    Operation::query( 'acme/list-reports' )
        ->input( null, S::object( [] ) )
        ->output( null, S::array( Type::ref( 'report' ) ) )
        ->public()
        ->register();
    wp_operation_transports( 'acme/list-reports', [ 'rest' ] );
}
add_action( 'wp_operations_init', 'acme_register' );
"#;

/// JSON for a schema PHP built: PHP writes an empty schema object (`S::any()`) as `[]`.
fn schema_json(value: &Value) -> serde_json::Value {
    match json(value) {
        serde_json::Value::Array(a) if a.is_empty() => serde_json::json!({}),
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(k, v)| {
                    let v = if k == "properties" {
                        match v {
                            serde_json::Value::Object(props) => serde_json::Value::Object(
                                props
                                    .into_iter()
                                    .map(|(n, p)| match p {
                                        serde_json::Value::Array(a) if a.is_empty() => {
                                            (n, serde_json::json!({}))
                                        }
                                        p => (n, p),
                                    })
                                    .collect(),
                            ),
                            v => v,
                        }
                    } else {
                        v
                    };
                    (k, v)
                })
                .collect(),
        ),
        other => other,
    }
}

fn thrown(result: &Result<PhpValue, speedyspec_php::PhpError>, contains: &str) -> bool {
    matches!(result, Ok(PhpValue::Str(m)) if m.contains(contains))
}

fn try_php(body: &str) -> String {
    format!(
        "try {{ {body} return 'ran'; }} catch ( \\SpeedySpec\\Operation\\RegistrationException $e ) {{ return $e->getMessage(); }}"
    )
}

fn main() {
    let site = SiteId {
        blog: 1,
        fingerprint: "a".into(),
    };
    let b = boot(identity(), Rc::new(FixedSiteKey(site)));
    load(&b.php, HOOKS);
    load(&b.php, FIXTURE);

    // php_schema_matches_rust_export
    let php_schema = eval(
        &b.php,
        r"return \SpeedySpec\Operation\S::object( [
            'id' => \SpeedySpec\Operation\S::integer()->required(),
            'at' => \SpeedySpec\Operation\S::dateTime()->nullable(),
            'file' => \SpeedySpec\Operation\S::binary( 'application/pdf' ),
            'any' => \SpeedySpec\Operation\S::any(),
            'post' => \SpeedySpec\Operation\Type::ref( 'post' )->nullable(),
        ] )->toArray();",
    )
    .expect("S builds");
    let as_value = neutral(&php_schema);
    let parsed = S::from_value(&as_value).map(|s| json_schema(&s.build()));
    check(
        "php_schema_matches_rust_export",
        parsed.as_ref().is_ok_and(|p| *p == schema_json(&as_value)),
        (&parsed, schema_json(&as_value)),
    );

    // operation_registers_and_documents
    let site = b.area.site().expect("the site initializes");
    let update = site.registries.get_operation("acme/update-report");
    let order = update.as_ref().map(|op| match &op.input.schema.kind {
        SchemaKind::Object { properties, .. } => properties.keys().cloned().collect::<Vec<_>>(),
        _ => vec![],
    });
    let document = eval(
        &b.php,
        r"$d = \SpeedySpec\Operation\Operations::document();
          return $d['operations']['acme/update-report']['transports'] ?? null;",
    );
    check(
        "operation_registers_and_documents",
        site.registries.is_closed()
            && order.as_deref() == Some(&["id".to_owned(), "title".into(), "file".into()][..])
            && matches!(&document, Ok(v) if neutral(v) == Value::List(vec![Value::from("rest"), Value::from("xmlrpc")])),
        (&order, &document, site.problems()),
    );

    // wp_operation_transports_restricts
    let list = site.registries.get_operation("acme/list-reports");
    check(
        "wp_operation_transports_restricts",
        list.as_ref()
            .is_some_and(|op| op.exposed_on("rest") && !op.exposed_on("xmlrpc")),
        list.map(|op| op.transports.clone()),
    );

    // mismatched_class_throws_registration_exception
    let mismatched = eval(
        &b.php,
        &try_php(
            r"\SpeedySpec\Operation\Operation::query( 'acme/bad' )
                ->input( BadInput::class, \SpeedySpec\Operation\S::object( [ 'id' => \SpeedySpec\Operation\S::integer()->required() ] ) )
                ->output( null, \SpeedySpec\Operation\S::object( [] ) )->public()->register();",
        ),
    );
    check(
        "mismatched_class_throws_registration_exception",
        thrown(&mismatched, "is string, the schema says integer"),
        &mismatched,
    );

    // missing_permission_throws
    let missing = eval(
        &b.php,
        &try_php(
            r"\SpeedySpec\Operation\Operation::query( 'acme/no-permission' )
                ->input( null, \SpeedySpec\Operation\S::object( [] ) )
                ->output( null, \SpeedySpec\Operation\S::object( [] ) )->register();",
        ),
    );
    check(
        "missing_permission_throws",
        thrown(&missing, "acme/no-permission"),
        &missing,
    );

    // duplicate_error_code_throws
    let duplicate = eval(
        &b.php,
        &try_php(
            r"\SpeedySpec\Operation\Operation::query( 'acme/dup' )
                ->input( null, \SpeedySpec\Operation\S::object( [] ) )
                ->output( null, \SpeedySpec\Operation\S::object( [] ) )
                ->error( 'x', \SpeedySpec\Operation\Category::Conflict )
                ->error( 'x', \SpeedySpec\Operation\Category::NotFound )->public()->register();",
        ),
    );
    check(
        "duplicate_error_code_throws",
        thrown(&duplicate, "x"),
        &duplicate,
    );

    // unknown_class_throws
    let unknown = eval(
        &b.php,
        &try_php(
            r"\SpeedySpec\Operation\Operation::query( 'acme/ghost' )
                ->input( 'Acme\Missing', \SpeedySpec\Operation\S::object( [] ) )
                ->output( null, \SpeedySpec\Operation\S::object( [] ) )->public()->register();",
        ),
    );
    check(
        "unknown_class_throws",
        thrown(&unknown, "does not exist"),
        &unknown,
    );

    // closed_registration_is_a_no_op_or_refused
    let same = eval(&b.php, &try_php("acme_register();"));
    let changed = eval(
        &b.php,
        &try_php(
            r"\SpeedySpec\Operation\Operation::query( 'acme/list-reports' )
                ->input( null, \SpeedySpec\Operation\S::object( [] ) )
                ->output( null, \SpeedySpec\Operation\S::array( \SpeedySpec\Operation\Type::ref( 'report' ) ) )
                ->deprecated( 'gone' )->public()->register();",
        ),
    );
    check(
        "closed_registration_is_a_no_op_or_refused",
        matches!(&same, Ok(PhpValue::Str(s)) if s == "ran") && thrown(&changed, "closed"),
        (&same, &changed),
    );
}
