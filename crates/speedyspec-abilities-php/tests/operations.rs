//! Operations as abilities: registered into WordPress's own registries, executed through their Rust steps.

mod support;

use speedyspec_php_ext::Value as PhpValue;
use support::*;

const PLUGIN: &str = r#"
use SpeedySpec\Operation\{S, Type, Operation, Handler, Caller};
use SpeedySpec\Abilities\Abilities;

function acme_can_read( Caller $caller, array $input ): bool { return $caller->can( 'edit_report' ); }
function acme_get_report( array $input, Caller $caller ): array { $GLOBALS['handled'] = true; return [ 'id' => $input['id'], 'title' => 'Q3' ]; }
function acme_delete_report( array $input, Caller $caller ): array { return [ 'deleted' => true ]; }
function acme_init(): void {
    Type::resource( 'report' )->field( 'id', S::integer()->required() )->register();
    Operation::query( 'acme/get-report' )->resource( 'report', 'get', 'id' )->label( 'Get report', 'Gets a report.' )
        ->input( null, S::object( [ 'id' => S::integer()->required() ] ) )->output( null, S::any() )
        ->permission( 'acme_can_read' )->register();
    Handler::for( 'acme/get-report', 'acme_get_report' );
    Operation::command( 'acme/delete-report' )->resource( 'report', 'delete', 'id' )
        ->input( null, S::object( [ 'id' => S::integer()->required() ] ) )->output( null, S::any() )->public()->register();
    Handler::for( 'acme/delete-report', 'acme_delete_report' );
    Abilities::publish( 'acme/delete-report' )->category( 'reports' )->label( 'Delete report' )->public( true );
    Operation::query( 'zeta/ping' )->input( null, S::object( [] ) )->output( null, S::any() )->public()->register();
    Handler::for( 'zeta/ping', 'acme_delete_report' );
    Operation::query( 'acme/rest-only' )->input( null, S::object( [] ) )->output( null, S::any() )->public()
        ->transports( 'rest' )->register();
    Handler::for( 'acme/rest-only', 'acme_delete_report' );
}
add_action( 'wp_operations_init', 'acme_init' );
function acme_categories(): void {
    wp_register_ability_category( 'reports', [ 'label' => 'Reports', 'description' => 'Report abilities.' ] );
    wp_register_ability_category( 'acme', [ 'label' => 'Acme', 'description' => 'Acme abilities.' ] );
}
add_action( 'wp_abilities_api_categories_init', 'acme_categories' );
function t_order( ...$args ) { $GLOBALS['order'][] = current_action(); return $args[0] ?? null; }
foreach ( [ 'wp_ability_invoked', 'wp_pre_execute_ability', 'wp_ability_normalize_input', 'wp_ability_validate_input',
    'wp_ability_permission_result', 'wp_before_execute_ability', 'wp_ability_execute_result',
    'wp_ability_validate_output', 'wp_after_execute_ability' ] as $speedyspec_hook ) {
    add_filter( $speedyspec_hook, 't_order', 10, 4 );
}
"#;

fn json(b: &Booted, expression: &str) -> String {
    match eval(&b.php, &format!("return json_encode( {expression} );")) {
        Ok(PhpValue::Str(s)) => s,
        other => format!("{other:?}"),
    }
}

fn main() {
    let b = boot_native();
    load_wordpress_native(&b.php);
    let root = release().join("wp-includes");
    load(
        &b.php,
        &format!(
            "if ( ! defined( 'ABSPATH' ) ) {{ define( 'ABSPATH', {:?} ); define( 'WPINC', 'wp-includes' ); }}",
            format!("{}/", release().display())
        ),
    );
    for file in [
        "abilities-api/class-wp-ability-category.php",
        "abilities-api/class-wp-ability-categories-registry.php",
        "abilities-api/class-wp-abilities-registry.php",
        "abilities-api.php",
    ] {
        load(
            &b.php,
            &format!("require_once {:?};", root.join(file).display().to_string()),
        );
    }
    load(&b.php, PLUGIN);
    load(&b.php, "do_action( 'init' );");

    let ability = json(
        &b,
        "( function () { $a = wp_get_ability( 'acme/get-report' ); return [ get_class( $a ), $a->get_label(), $a->get_description(),
            $a->get_category(), $a->get_meta(), $a->get_input_schema()['properties']['id']['type'] ?? null,
            wp_has_ability_category( 'acme' ) ]; } )()",
    );
    check(
        "operations_are_registered_as_abilities",
        ability
            == r#"["SpeedySpec\\Abilities\\OperationAbility","Get report","Gets a report.","acme",{"annotations":{"readonly":true,"destructive":false,"idempotent":true},"show_in_rest":true,"public":false},"integer",true]"#,
        &ability,
    );

    let published = json(
        &b,
        "( function () { $a = wp_get_ability( 'acme/delete-report' ); return [ $a->get_label(), $a->get_category(), $a->get_meta() ]; } )()",
    );
    check(
        "published_metadata_wins",
        published
            == r#"["Delete report","reports",{"annotations":{"readonly":false,"destructive":true,"idempotent":true},"show_in_rest":true,"public":true}]"#,
        &published,
    );

    let restricted = json(&b, "wp_has_ability( 'acme/rest-only' )");
    check(
        "restricted_operations_are_not_abilities",
        restricted == "false",
        &restricted,
    );

    load(&b.php, "$GLOBALS['uid'] = 7; $GLOBALS['order'] = [];");
    let executed = json(
        &b,
        "wp_get_ability( 'acme/get-report' )->execute( [ 'id' => 4 ] )",
    );
    check(
        "executing_an_operation_ability_runs_the_operation",
        executed == r#"{"id":4,"title":"Q3"}"# && global(&b.php, "handled") == PhpValue::Bool(true),
        &executed,
    );
    check(
        "hooks_fire_for_operation_abilities_too",
        json(&b, "$GLOBALS['order']")
            == r#"["wp_ability_invoked","wp_pre_execute_ability","wp_ability_normalize_input","wp_ability_validate_input","wp_ability_permission_result","wp_before_execute_ability","wp_ability_execute_result","wp_ability_validate_output","wp_after_execute_ability"]"#,
        json(&b, "$GLOBALS['order']"),
    );

    let invalid = json(
        &b,
        "( function () { $r = wp_get_ability( 'acme/get-report' )->execute( [ 'id' => 'x' ] ); return [ $r->get_error_code(), str_starts_with( $r->get_error_message(), 'Ability \"acme/get-report\" has invalid input. Reason: ' ) ]; } )()",
    );
    load(&b.php, "$GLOBALS['uid'] = 8;");
    let refused = json(
        &b,
        "( function () { $r = wp_get_ability( 'acme/get-report' )->execute( [ 'id' => 4 ] ); return $r->get_error_code(); } )()",
    );
    check(
        "an_operation_ability_s_errors_are_wordpress_s",
        invalid == r#"["ability_invalid_input",true]"#
            && refused == r#""ability_invalid_permissions""#,
        (&invalid, &refused),
    );

    let filtered = json(
        &b,
        "[ array_keys( wp_get_abilities( [ 'category' => 'acme' ] ) ), array_keys( wp_get_abilities( [ 'meta' => [ 'annotations' => [ 'readonly' => false ] ] ] ) ) ]",
    );
    check(
        "wp_get_abilities_filters_operation_abilities",
        filtered == r#"[["acme\/get-report"],["acme\/delete-report"]]"#,
        &filtered,
    );

    let categories = json(
        &b,
        "[ wp_get_ability_category( 'acme' )->get_label(), wp_get_ability_category( 'zeta' )->get_label(), $GLOBALS['__doing_it_wrong'] ?? [] ]",
    );
    check(
        "a_plugin_s_own_vendor_category_wins",
        categories == r#"["Acme","zeta",[]]"#,
        &categories,
    );

    load(&b.php, "$GLOBALS['uid'] = 7;");
    let direct = json(
        &b,
        "( function () { $a = wp_get_ability( 'acme/get-report' ); $in = $a->normalize_input( [ 'id' => 4 ] );
            $bad = $a->validate_input( [ 'id' => 'x' ] );
            return [ $in, $a->validate_input( $in ), $a->check_permissions( $in ), is_wp_error( $bad ) ? $bad->get_error_code() : $bad ]; } )()",
    );
    load(&b.php, "$GLOBALS['uid'] = 8;");
    let refused = json(
        &b,
        "wp_get_ability( 'acme/get-report' )->check_permissions( [ 'id' => 4 ] )",
    );
    check(
        "an_operation_ability_s_steps_work_when_called_directly",
        direct == r#"[{"id":4},true,true,"ability_invalid_input"]"# && refused == "false",
        (&direct, &refused),
    );
}
