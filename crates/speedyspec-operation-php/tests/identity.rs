//! `Caller` in PHP, and the `user_can` Identity adapter.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`).

mod support;

use std::rc::Rc;

use speedyspec_operation_contracts::{Caller, Identity, Value};
use speedyspec_operation_php::{FixedSiteKey, PhpIdentity, SiteId, php_caller};
use speedyspec_php_ext::{PhpObject, Value as PhpValue};
use support::*;

const FIXTURE: &str = r#"
function user_can( $user, $cap, ...$args ) { return 7 === $user && 'edit_report' === $cap && [ 3 ] === $args; }
function caller_user( \SpeedySpec\Operation\Caller $caller ) { return $caller->user; }
function caller_transport( \SpeedySpec\Operation\Caller $caller ) { return $caller->transport; }
"#;

fn main() {
    let identity: Rc<dyn Identity> = Rc::new(PhpIdentity::new());
    let b = boot(
        identity.clone(),
        Rc::new(FixedSiteKey(SiteId {
            blog: 1,
            fingerprint: String::new(),
        })),
    );
    load(&b.php, FIXTURE);

    // php_identity_calls_user_can
    let php = PhpIdentity::new();
    let three = [Value::Int(3)];
    check(
        "php_identity_calls_user_can",
        php.can(Some(7), "edit_report", &three)
            && !php.can(Some(8), "edit_report", &three)
            && !php.can(None, "edit_report", &three)
            && !php.can(Some(7), "edit_report", &[]),
        (),
    );

    // missing_function_denies
    check(
        "missing_function_denies",
        !PhpIdentity::with_function("nope").can(Some(7), "x", &[]),
        (),
    );

    // php_caller_can_reaches_identity
    let caller = php_caller(&Caller::new(Some(7), "internal", identity));
    let object = caller.as_ref().ok().and_then(|v| PhpObject::new(v.clone()));
    let can = object.as_ref().map(|o| {
        o.call(
            "can",
            vec![PhpValue::Str("edit_report".into()), PhpValue::Int(3)],
        )
    });
    let cannot = object.as_ref().map(|o| {
        o.call(
            "can",
            vec![PhpValue::Str("edit_report".into()), PhpValue::Int(4)],
        )
    });
    let user = caller.as_ref().map(|c| {
        speedyspec_php::call(
            &PhpValue::Str("caller_user".into()),
            std::slice::from_ref(c),
        )
    });
    let transport = caller.as_ref().map(|c| {
        speedyspec_php::call(
            &PhpValue::Str("caller_transport".into()),
            std::slice::from_ref(c),
        )
    });
    check(
        "php_caller_can_reaches_identity",
        matches!(can, Some(Ok(PhpValue::Bool(true))))
            && matches!(cannot, Some(Ok(PhpValue::Bool(false))))
            && matches!(user, Ok(Ok(PhpValue::Int(7))))
            && matches!(&transport, Ok(Ok(PhpValue::Str(t))) if t == "internal"),
        (&can, &cannot, &user, &transport),
    );
    let anonymous = php_caller(&Caller::new(None, "rest", Rc::new(PhpIdentity::new())))
        .map(|c| speedyspec_php::call(&PhpValue::Str("caller_user".into()), &[c]));
    check(
        "anonymous_caller_has_no_user",
        matches!(anonymous, Ok(Ok(PhpValue::Null))),
        &anonymous,
    );
}
