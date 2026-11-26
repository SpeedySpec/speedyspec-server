mod rig;

use rig::*;
use speedyspec_native_stage::{NativeResponse, StageOutcome};
use speedyspec_wp_hooks_contracts::HookValue;

fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn blocked_request_gets_exact_403() {
    let rig = Rig::new(&[("block", HookValue::Str("192.0.2.0/24".into()))]);
    assert_eq!(
        answer(&rig, "192.0.2.7", "GET", "/"),
        StageOutcome::Respond(NativeResponse {
            status: 403,
            headers: headers(&[
                ("Cache-Control", "no-store"),
                ("Content-Type", "text/plain; charset=utf-8")
            ]),
            body: b"Forbidden\n".to_vec(),
        })
    );
}

#[test]
fn rate_limited_request_gets_429_with_retry_after() {
    let rig = Rig::new(&[
        ("rate_requests", HookValue::Int(1)),
        ("rate_ban", HookValue::Int(90)),
    ]);
    assert_eq!(
        answer(&rig, "192.0.2.7", "GET", "/"),
        StageOutcome::Continue
    );
    assert_eq!(
        answer(&rig, "192.0.2.7", "GET", "/"),
        StageOutcome::Respond(NativeResponse {
            status: 429,
            headers: headers(&[
                ("Cache-Control", "no-store"),
                ("Content-Type", "text/plain; charset=utf-8"),
                ("Retry-After", "90"),
            ]),
            body: b"Too Many Requests\n".to_vec(),
        })
    );
}

#[test]
fn missing_remote_addr_continues_and_logs() {
    let rig = Rig::new(&[("block", HookValue::Str("0.0.0.0/0".into()))]);
    assert_eq!(rig.answer(&[("REQUEST_URI", "/")]), StageOutcome::Continue);
    assert!(
        rig.log
            .messages
            .borrow()
            .iter()
            .any(|m| m.contains("REMOTE_ADDR"))
    );
}

#[test]
fn allowed_request_continues() {
    let rig = Rig::new(&[("block", HookValue::Str("198.51.100.0/24".into()))]);
    assert_eq!(
        answer(&rig, "192.0.2.7", "GET", "/"),
        StageOutcome::Continue
    );
}

#[test]
fn stage_is_named_firewall() {
    use speedyspec_native_stage::NativeStage;
    let rig = Rig::new(&[]);
    assert_eq!(rig.stage.name(), "firewall");
}
