//! WordPress's ability hooks, WP_Error mapping and the handles of abilities.

mod support;

use speedyspec_abilities::errors::wp_error;
use speedyspec_abilities_contracts::AbilityHooks;
use speedyspec_abilities_php::{PhpAbilityHooks, errors, handles};
use speedyspec_operation_contracts::{Category, Value};
use speedyspec_php_ext::Value as PhpValue;
use support::*;

const FIXTURE: &str = r#"
$GLOBALS['the_ability'] = new stdClass();
function t_record( ...$args ) {
    $GLOBALS['seen'][] = [ current_filter_name(), $args[0] ?? null, end( $args ) === $GLOBALS['the_ability'] ];
}
function current_filter_name() { return $GLOBALS['__current'] ?? ''; }
function t_invoked( $name, $input, $ability ) { $GLOBALS['seen'][] = [ 'invoked', $name, $input, $ability === $GLOBALS['the_ability'] ]; }
add_action( 'wp_ability_invoked', 't_invoked', 10, 3 );
function t_pre( $pre, $name, $input, $ability ) {
    $GLOBALS['seen'][] = [ 'pre', $pre instanceof WP_Filter_Sentinel ];
    if ( array_key_exists( 'pre', $GLOBALS ) ) {
        return $GLOBALS['pre'];
    }
    return $pre;
}
add_filter( 'wp_pre_execute_ability', 't_pre', 10, 4 );
function t_permission( $permission, $name, $input, $ability ) {
    return $GLOBALS['permission'] ?? $permission;
}
add_filter( 'wp_ability_permission_result', 't_permission', 10, 4 );
function t_validate( $valid, $input, $name ) {
    return array_key_exists( 'valid', $GLOBALS ) ? $GLOBALS['valid'] : $valid;
}
add_filter( 'wp_ability_validate_input', 't_validate', 10, 3 );
add_filter( 'wp_ability_validate_output', 't_validate', 10, 3 );
function t_normalize( $input, $name, $ability ) {
    if ( ! empty( $GLOBALS['explode'] ) ) {
        throw new RuntimeException( 'a plugin broke' );
    }
    return is_array( $input ) ? $input + [ 'normalized' => true ] : $input;
}
add_filter( 'wp_ability_normalize_input', 't_normalize', 10, 3 );
function t_after( $name, $input, $result, $ability ) { $GLOBALS['seen'][] = [ 'after', $name, $result, $ability === $GLOBALS['the_ability'] ]; }
add_action( 'wp_after_execute_ability', 't_after', 10, 4 );
"#;

fn json(b: &Booted, expression: &str) -> String {
    match eval(&b.php, &format!("return json_encode( {expression} );")) {
        Ok(PhpValue::Str(s)) => s,
        other => format!("{other:?}"),
    }
}

fn main() {
    let b = boot();
    load_wordpress(&b.php);
    load(&b.php, FIXTURE);
    let ability = eval(&b.php, "return $GLOBALS['the_ability'];").unwrap();
    let handle = handles::add(ability);
    let hooks = PhpAbilityHooks::new();
    let input = Some(Value::map([("a", Value::Int(1))]));

    let invoked = hooks.invoked("acme/x", &input, handle);
    let after = hooks.after_execute("acme/x", &input, &Value::Int(5), handle);
    check(
        "hook_arguments_are_wordpress_s",
        invoked.is_ok()
            && after.is_ok()
            && json(&b, "$GLOBALS['seen']")
                == r#"[["invoked","acme\/x",{"a":1},true],["after","acme\/x",5,true]]"#,
        json(&b, "$GLOBALS['seen']"),
    );

    load(&b.php, "$GLOBALS['seen'] = [];");
    let run = hooks.pre_execute("acme/x", &input, handle);
    check(
        "pre_execute_runs_when_the_sentinel_comes_back",
        matches!(run, Ok(None)) && json(&b, "$GLOBALS['seen']") == r#"[["pre",true]]"#,
        &run,
    );
    load(&b.php, "$GLOBALS['pre'] = null;");
    let short = hooks.pre_execute("acme/x", &input, handle);
    check(
        "pre_execute_compares_by_identity",
        matches!(short, Ok(Some(Ok(Value::Null)))),
        &short,
    );
    load(
        &b.php,
        "$GLOBALS['pre'] = new WP_Error( 'blocked', 'Blocked.', [ 'status' => 403 ] );",
    );
    let blocked = hooks.pre_execute("acme/x", &input, handle);
    check(
        "pre_execute_can_answer_with_an_error",
        matches!(&blocked, Ok(Some(Err(e))) if e.code == "blocked" && e.message == "Blocked."),
        &blocked,
    );
    load(&b.php, "unset( $GLOBALS['pre'] );");

    let normalized = hooks.normalize_input("acme/x", input.clone(), handle);
    check(
        "normalize_input_filters_the_input",
        normalized
            == Ok(Some(Value::map([
                ("a", Value::Int(1)),
                ("normalized", Value::Bool(true)),
            ]))),
        &normalized,
    );

    load(&b.php, "$GLOBALS['permission'] = 'yes';");
    let coerced = hooks.permission_result("acme/x", Ok(true), &input, handle);
    load(&b.php, "unset( $GLOBALS['permission'] );");
    let kept = hooks.permission_result("acme/x", Ok(true), &input, handle);
    check(
        "permission_result_coerces_as_wordpress_does",
        coerced == Ok(false) && kept == Ok(true),
        (&coerced, &kept),
    );

    load(&b.php, "$GLOBALS['valid'] = false;");
    let input_false = hooks.validate_input("acme/x", Ok(()), &input);
    let output_false = hooks.validate_output("acme/x", Ok(()), &Value::Int(1));
    load(&b.php, "unset( $GLOBALS['valid'] );");
    check(
        "validate_filters_turn_false_into_wordpress_s_errors",
        input_false
            .as_ref()
            .is_err_and(|e| e.code == "ability_invalid_input" && e.message == "Invalid input.")
            && output_false.as_ref().is_err_and(|e| {
                e.code == "ability_invalid_output" && e.message == "Invalid output."
            }),
        (&input_false, &output_false),
    );
    let still_invalid = hooks.validate_input(
        "acme/x",
        Err(wp_error("ability_invalid_input", "Bad.")),
        &input,
    );
    check(
        "an_invalid_result_passes_through_the_filter",
        still_invalid.as_ref().is_err_and(|e| e.message == "Bad."),
        &still_invalid,
    );

    let error = errors::from_wp_error(
        &eval(
            &b.php,
            "return new WP_Error( 'acme_nope', 'Nope.', [ 'status' => 403, 'why' => 'x' ] );",
        )
        .unwrap(),
    )
    .unwrap();
    let back = errors::to_wp_error(&error);
    let back_json = speedyspec_php::call(
        &PhpValue::Str("json_encode".into()),
        &[speedyspec_php::call(
            &PhpValue::Str("SpeedySpec\\Abilities\\Internal\\Adapter::describe_error".into()),
            &[back],
        )
        .unwrap()],
    );
    check(
        "a_wp_error_round_trips_exactly",
        error.code == "acme_nope"
            && error.message == "Nope."
            && error.category == Category::PermissionDenied
            && matches!(&back_json, Ok(PhpValue::Str(s)) if s == r#"["acme_nope","Nope.",{"status":403,"why":"x"}]"#),
        (&error, &back_json),
    );

    hooks.doing_it_wrong("WP_Ability::execute", "No <b>way</b>.", "6.9.0");
    check(
        "doing_it_wrong_escapes_as_wordpress_does",
        json(&b, "$GLOBALS['__doing_it_wrong']")
            == r#"["WP_Ability::execute: No &lt;b&gt;way&lt;\/b&gt;. (6.9.0)"]"#,
        json(&b, "$GLOBALS['__doing_it_wrong']"),
    );

    load(&b.php, "$GLOBALS['explode'] = true;");
    let exploded = hooks.normalize_input("acme/x", input.clone(), handle);
    check(
        "a_throwing_hook_fails_closed",
        exploded
            .as_ref()
            .is_err_and(|e| e.code == "ability_hook_failed" && e.category == Category::Internal),
        &exploded,
    );
}
