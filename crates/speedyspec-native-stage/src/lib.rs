//! The contract for answering a request in Rust before PHP runs.
//!
//! The runtime builds a [`StageChain`] once per worker and asks it about every request before it starts the PHP
//! script. A stage either lets the request through ([`StageOutcome::Continue`]) or answers it
//! ([`StageOutcome::Respond`]), and then PHP never runs for that request.
//!
//! A stage sees the request's FastCGI params only, never its body: reading the body here would take it away from
//! PHP, which may still need it.

use std::rc::Rc;

/// What a stage may look at: the request's FastCGI params, in the order they arrived.
pub struct StageRequest<'a> {
    pub params: &'a [(String, String)],
}

impl StageRequest<'_> {
    /// The value of the param named exactly `name` (names are case-sensitive), or `None`.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// A complete response written by a stage in place of PHP's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// A stage's answer about one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageOutcome {
    /// Let the next stage, and in the end PHP, handle the request.
    Continue,
    /// Answer the request with this response; nothing after this stage runs.
    Respond(NativeResponse),
}

/// One step that may answer a request before PHP runs.
pub trait NativeStage {
    fn name(&self) -> &str;
    fn answer(&self, request: &StageRequest<'_>) -> StageOutcome;
}

/// Stages asked in order (Chain of Responsibility): the first that responds wins.
#[derive(Default)]
pub struct StageChain {
    stages: Vec<Rc<dyn NativeStage>>,
}

impl StageChain {
    pub fn new() -> StageChain {
        StageChain::default()
    }

    pub fn push(&mut self, stage: Rc<dyn NativeStage>) {
        self.stages.push(stage);
    }

    pub fn answer(&self, request: &StageRequest<'_>) -> StageOutcome {
        for stage in &self.stages {
            if let outcome @ StageOutcome::Respond(_) = stage.answer(request) {
                return outcome;
            }
        }
        StageOutcome::Continue
    }
}
