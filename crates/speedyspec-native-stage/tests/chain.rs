use std::cell::Cell;
use std::rc::Rc;

use speedyspec_native_stage::{NativeResponse, NativeStage, StageChain, StageOutcome, StageRequest};

struct Counting {
    calls: Cell<u32>,
    respond: bool,
}

impl NativeStage for Counting {
    fn name(&self) -> &str {
        "counting"
    }

    fn answer(&self, _request: &StageRequest<'_>) -> StageOutcome {
        self.calls.set(self.calls.get() + 1);
        if self.respond {
            StageOutcome::Respond(NativeResponse { status: 403, headers: Vec::new(), body: b"no\n".to_vec() })
        } else {
            StageOutcome::Continue
        }
    }
}

fn params() -> Vec<(String, String)> {
    vec![("REMOTE_ADDR".to_string(), "192.0.2.1".to_string())]
}

#[test]
fn empty_chain_continues() {
    let params = params();
    let chain = StageChain::new();
    assert!(matches!(chain.answer(&StageRequest { params: &params }), StageOutcome::Continue));
}

#[test]
fn first_respond_wins_and_later_stages_are_not_called() {
    let params = params();
    let passing = Rc::new(Counting { calls: Cell::new(0), respond: false });
    let blocking = Rc::new(Counting { calls: Cell::new(0), respond: true });
    let after = Rc::new(Counting { calls: Cell::new(0), respond: true });
    let mut chain = StageChain::new();
    chain.push(passing.clone());
    chain.push(blocking.clone());
    chain.push(after.clone());

    match chain.answer(&StageRequest { params: &params }) {
        StageOutcome::Respond(response) => assert_eq!(response.status, 403),
        StageOutcome::Continue => panic!("expected a response"),
    }
    assert_eq!(passing.calls.get(), 1);
    assert_eq!(blocking.calls.get(), 1);
    assert_eq!(after.calls.get(), 0);
}

#[test]
fn param_finds_by_exact_name() {
    let params = params();
    let request = StageRequest { params: &params };
    assert_eq!(request.param("REMOTE_ADDR"), Some("192.0.2.1"));
    assert_eq!(request.param("remote_addr"), None);
}
