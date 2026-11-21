use speedyspec_http::Method;

use crate::{Params, RouteId, RouteName};

/// A query variable's value as PHP's `parse_str` produces it: text, or a nested map for `a[b]=` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryValue {
    Text(String),
    Map(Vec<(String, QueryValue)>),
}

/// Query variables in the order PHP would hold them.
pub type QueryVars = Vec<(String, QueryValue)>;

/// A WordPress rewrite rule that matched, as `WP::parse_request` records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleMatch {
    pub matched_rule: String,
    pub matched_query: String,
    pub query_vars: QueryVars,
}

/// What matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteMatch {
    /// A registered route.
    Route {
        id: RouteId,
        name: Option<RouteName>,
        params: Params,
    },
    /// A WordPress rule.
    Rule(RuleMatch),
    /// A rule matched whose guard cannot be answered here (no Rust adapter yet); PHP finishes from this rule.
    PendingGuard { rule_index: usize },
    /// Matching reached a rule no Rust regex engine compiles; PHP matches from this rule.
    NeedsPhp { rule_index: usize },
}

/// What resolving a request answers: a route, a redirect, or not found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Route(RouteMatch),
    Redirect {
        location: String,
        status: u16,
    },
    /// Nothing matched. `allowed` lists the methods the path matches under, for a 405, or is empty.
    NotFound {
        allowed: Vec<Method>,
    },
}
