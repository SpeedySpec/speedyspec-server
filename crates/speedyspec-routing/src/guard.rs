use crate::Rule;

/// A guard's answer about a matched rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardAnswer {
    Accept,
    /// Not this rule: matching resumes at the next one.
    Reject,
    /// The guard needs something this side cannot answer (data not ported to Rust yet).
    Unavailable,
}

/// A condition a matched rule must also meet (Specification), such as WordPress's check that a verbose page rule
/// names a page that exists.
pub trait RuleGuard {
    fn check(&self, rule: &Rule, captures: &[Option<String>]) -> GuardAnswer;
}
