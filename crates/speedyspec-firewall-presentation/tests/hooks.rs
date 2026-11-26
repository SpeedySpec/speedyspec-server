mod rig;

use rig::*;
use speedyspec_native_stage::StageOutcome;
use speedyspec_wp_hooks_contracts::{HookName, HookValue};

fn failed_login(rig: &Rig) {
    rig.hooks
        .do_action(
            &HookName::from("wp_login_failed"),
            vec![HookValue::Str("admin".into()), HookValue::Null],
        )
        .unwrap();
}

fn status(outcome: StageOutcome) -> Option<u16> {
    match outcome {
        StageOutcome::Respond(response) => Some(response.status),
        StageOutcome::Continue => None,
    }
}

#[test]
fn login_failed_hook_locks_out_after_max() {
    let rig = Rig::new(&[]);
    assert_eq!(
        answer(&rig, "192.0.2.9", "POST", "/wp-login.php"),
        StageOutcome::Continue
    );
    for _ in 0..5 {
        failed_login(&rig);
    }
    assert_eq!(
        status(answer(&rig, "192.0.2.9", "POST", "/wp-login.php")),
        Some(429)
    );
    assert_eq!(
        answer(&rig, "192.0.2.10", "POST", "/wp-login.php"),
        StageOutcome::Continue
    );
}

#[test]
fn login_failed_without_a_current_client_does_nothing() {
    let rig = Rig::new(&[("login_max_failures", HookValue::Int(1))]);
    failed_login(&rig);
    assert_eq!(
        answer(&rig, "192.0.2.9", "POST", "/wp-login.php"),
        StageOutcome::Continue
    );
}

#[test]
fn xmlrpc_enabled_filter_follows_setting() {
    let filter = |rig: &Rig| {
        rig.hooks
            .apply_filters(
                &HookName::from("xmlrpc_enabled"),
                HookValue::Bool(true),
                Vec::new(),
            )
            .unwrap()
    };
    let off = Rig::new(&[]);
    assert!(matches!(filter(&off), HookValue::Bool(true)));
    let on = Rig::new(&[("block_xmlrpc", HookValue::Bool(true))]);
    assert!(matches!(filter(&on), HookValue::Bool(false)));
}
