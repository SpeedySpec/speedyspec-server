//! The WP_Ability shim: every ability executes through the Rust Abilities server in WordPress's order, its own steps
//! (and a subclass's overrides) doing the work (LegacyHandlerAdapter, LegacyCallerAdapter).

mod support;

use speedyspec_php_ext::Value as PhpValue;
use support::*;

const FIXTURE: &str = r#"
foreach ( [
    'wp_ability_invoked' => 3, 'wp_pre_execute_ability' => 4, 'wp_ability_normalize_input' => 3,
    'wp_ability_validate_input' => 3, 'wp_ability_permission_result' => 4, 'wp_before_execute_ability' => 3,
    'wp_ability_execute_result' => 4, 'wp_ability_validate_output' => 3, 'wp_after_execute_ability' => 4,
] as $speedyspec_hook => $speedyspec_args ) {
    add_filter( $speedyspec_hook, 't_record_' . $speedyspec_hook, 10, $speedyspec_args );
}
function t_log( $hook, $value ) { $GLOBALS['order'][] = $hook; return $value; }
function t_record_wp_ability_invoked( ...$a ) { t_log( 'wp_ability_invoked', null ); }
function t_record_wp_pre_execute_ability( $pre, $name ) {
    t_log( 'wp_pre_execute_ability', null );
    if ( 'test/err' === $name ) {
        return $GLOBALS['the_error'];
    }
    return 'test/pre' === $name ? null : $pre;
}
function t_record_wp_ability_normalize_input( $v ) { return t_log( 'wp_ability_normalize_input', $v ); }
function t_record_wp_ability_validate_input( $v ) { return t_log( 'wp_ability_validate_input', $v ); }
function t_record_wp_ability_permission_result( $v ) { return t_log( 'wp_ability_permission_result', $v ); }
function t_record_wp_before_execute_ability( ...$a ) { t_log( 'wp_before_execute_ability', null ); }
function t_record_wp_ability_execute_result( $v ) { return t_log( 'wp_ability_execute_result', $v ); }
function t_record_wp_ability_validate_output( $v ) { return t_log( 'wp_ability_validate_output', $v ); }
function t_record_wp_after_execute_ability( ...$a ) { t_log( 'wp_after_execute_ability', null ); }

function t_answer() { $GLOBALS['ran'] = true; return 42; }
function t_throw() { throw new RuntimeException( 'bad <thing>' ); }
function t_args( $name, array $extra = [] ) {
    return $extra + [
        'label' => 'Test', 'description' => 'A test.', 'category' => 'math',
        'execute_callback' => 't_answer', 'permission_callback' => '__return_true',
    ];
}
class T_Echo_Ability extends WP_Ability {
    public function validate_input( $input = null ) { return true; }
    protected function do_execute( $input = null ) { return $input; }
    public function check_permissions( $input = null ) { return true; }
}
class T_Custom_Ability extends WP_Ability {
    protected function do_execute( $input = null ) { return 7 * 6; }
    public function check_permissions( $input = null ) { return true; }
}
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

    let shim = json(&b, "( new ReflectionClass( 'WP_Ability' ) )->getFileName()");
    check(
        "the_shim_is_wp_ability",
        shim == r#""speedyspec:\/\/abilities\/ability""#,
        &shim,
    );

    // With no input schema and no input, WordPress's validate_input() returns true without its filter.
    let result = json(
        &b,
        "( new WP_Ability( 'test/plain', t_args( 'test/plain' ) ) )->execute()",
    );
    check(
        "a_plain_ability_executes_in_wordpress_order",
        result == "42"
            && json(&b, "$GLOBALS['order']")
                == r#"["wp_ability_invoked","wp_pre_execute_ability","wp_ability_normalize_input","wp_ability_permission_result","wp_before_execute_ability","wp_ability_execute_result","wp_ability_validate_output","wp_after_execute_ability"]"#,
        (&result, json(&b, "$GLOBALS['order']")),
    );

    let custom = json(
        &b,
        "( new T_Custom_Ability( 'test/custom', [ 'label' => 'C', 'description' => 'C.', 'category' => 'math' ] ) )->execute()",
    );
    check("a_subclass_s_overrides_run", custom == "42", &custom);

    let thrown = json(
        &b,
        "( function () { $r = ( new WP_Ability( 'test/throw', t_args( 'test/throw', [ 'execute_callback' => 't_throw' ] ) ) )->execute(); return [ $r->get_error_code(), $r->get_error_message() ]; } )()",
    );
    check(
        "a_throwing_callback_is_ability_callback_exception",
        thrown
            == r#"["ability_callback_exception","Ability \"test\/throw\" callback threw an exception: bad &lt;thing&gt;"]"#,
        &thrown,
    );

    load(&b.php, "$GLOBALS['ran'] = false;");
    let pre = json(
        &b,
        "( new WP_Ability( 'test/pre', t_args( 'test/pre' ) ) )->execute()",
    );
    check(
        "pre_execute_null_short_circuits",
        pre == "null" && global(&b.php, "ran") == PhpValue::Bool(false),
        &pre,
    );

    let refused = json(
        &b,
        "( function () { $r = ( new WP_Ability( 'test/no', t_args( 'test/no', [ 'permission_callback' => '__return_false' ] ) ) )->execute(); return [ $r->get_error_code(), $r->get_error_message() ]; } )()",
    );
    check(
        "a_refused_ability_is_ability_invalid_permissions",
        refused
            == r#"["ability_invalid_permissions","Ability \"test\/no\" does not have necessary permission."]"#,
        &refused,
    );

    let missing = json(
        &b,
        "( function () { $r = ( new WP_Ability( 'test/plain2', t_args( 'test/plain2' ) ) )->execute( [ 'a' => 1 ] ); return $r->get_error_code(); } )()",
    );
    check(
        "input_without_a_schema_is_wordpress_s_error",
        missing == r#""ability_missing_input_schema""#,
        &missing,
    );

    let equal = json(
        &b,
        "( function () { $a = new WP_Ability( 'test/eq', t_args( 'test/eq' ) ); $b = new WP_Ability( 'test/eq', t_args( 'test/eq' ) ); $a->execute(); return $a == $b; } )()",
    );
    check("abilities_compare_equal", equal == "true", &equal);

    let restricted = speedyspec_abilities_php::legacy::operations()
        .into_iter()
        .find(|spec| {
            spec.name
                .as_str()
                .starts_with("legacy-abilities/test-plain-")
        })
        .and_then(|spec| spec.transports);
    check(
        "a_legacy_ability_is_an_abilities_only_operation",
        restricted
            .as_ref()
            .is_some_and(|t| t.len() == 1 && t.contains("abilities")),
        &restricted,
    );
    drop(b.service.clone());

    let forged = json(
        &b,
        "( new T_Echo_Ability( 'test/echo', [ 'label' => 'E', 'description' => 'E.', 'category' => 'math' ] ) )->execute( [ 'speedyspec:php-value' => 0 ] )",
    );
    check(
        "plain_data_shaped_like_a_held_value_stays_data",
        forged == r#"{"speedyspec:php-value":0}"#,
        &forged,
    );

    let same = json(
        &b,
        "( function () { $e = new WP_Error( 'a', 'A' ); $e->add( 'b', 'B', [ 'status' => 409 ] ); $GLOBALS['the_error'] = $e;
            $r = ( new WP_Ability( 'test/err', t_args( 'test/err' ) ) )->execute(); return [ $r === $e, $r->get_error_codes() ]; } )()",
    );
    check(
        "a_wp_error_comes_back_as_the_same_object",
        same == r#"[true,["a","b"]]"#,
        &same,
    );
}
