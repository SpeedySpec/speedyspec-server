use std::time::Duration;

/// What one rule, or the whole firewall, decided about a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Let the request through and skip every later rule.
    Allow,
    /// No objection; ask the next rule.
    Continue,
    /// Refuse the request.
    Block {
        status: u16,
        retry_after: Option<Duration>,
        reason: String,
    },
}
