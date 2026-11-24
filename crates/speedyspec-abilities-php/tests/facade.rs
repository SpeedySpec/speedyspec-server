//! `Abilities::publish`.

mod support;

use speedyspec_operation_contracts::Value;
use speedyspec_php_ext::Value as PhpValue;
use speedyspec_transport_contracts::BindingStore;
use support::*;

fn main() {
    let b = boot();
    load(&b.php, HOOKS);

    load(
        &b.php,
        "SpeedySpec\\Abilities\\Abilities::publish( 'acme/get-report' )->category( 'reports' )->label( 'Get report' )
            ->public( true )->annotations( [ 'readonly' => true ] )->meta( [ 'foo' => 'bar' ] );",
    );
    let stored = b.bindings.binding("acme/get-report", "abilities");
    check(
        "publish_stores_the_binding",
        stored
            == Some(Value::map([
                ("label", Value::from("Get report")),
                ("category", Value::from("reports")),
                ("public", Value::Bool(true)),
                ("annotations", Value::map([("readonly", Value::Bool(true))])),
                ("meta", Value::map([("foo", Value::from("bar"))])),
            ])),
        &stored,
    );

    for (case, code) in [
        ("bad_category", "->category( 'Reports' )"),
        (
            "unknown_annotation",
            "->annotations( [ 'sparkly' => true ] )",
        ),
    ] {
        let refused = eval(
            &b.php,
            &format!(
                "try {{ SpeedySpec\\Abilities\\Abilities::publish( 'acme/{case}' ){code}; return 'stored'; }}
                 catch ( SpeedySpec\\Operation\\RegistrationException $e ) {{ return 'refused'; }}"
            ),
        );
        check(
            &format!("publish_refuses_{case}"),
            matches!(&refused, Ok(PhpValue::Str(s)) if s == "refused"),
            &refused,
        );
    }
}
