//! WordPress's rules at request time, matched through the router.

use std::rc::Rc;

use speedyspec_http::Method;
use speedyspec_router::{CompiledTable, RouteRequest, Router, RuleSlot, TableCache, WpSubject};
use speedyspec_routing::{Resolution, RouteMatch, Rule, RuleGuard, RuleMatch};

use crate::{RewriteError, RewriteOptions};

/// What matching a request against WordPress's rules gives `WP::parse_request`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WpOutcome {
    /// There are no rules (plain permalinks): `parse_request` does no permalink matching.
    NoRewrite,
    /// No rule matched: a 404 unless something else claims the request.
    NoMatch,
    Matched(RuleMatch),
    /// A rule matched whose guard only PHP can answer; PHP finishes from this rule.
    PendingGuard(usize),
    /// A rule no Rust engine compiles was reached; PHP matches from this rule.
    NeedsPhp(usize),
}

/// The `rewrite_rules` option as a router rule set, compiled once per distinct value and cached by fingerprint, so a
/// flush in any worker reaches this one at its next request without anything passing between them.
pub struct WpRouting {
    options: Rc<dyn RewriteOptions>,
    cache: Rc<TableCache>,
}

impl WpRouting {
    pub fn new(options: Rc<dyn RewriteOptions>, cache: Rc<TableCache>) -> WpRouting {
        WpRouting { options, cache }
    }

    /// The stored rules; `None` when the option is empty or not an array.
    pub fn rule_set(&self) -> Result<Option<Vec<Rule>>, RewriteError> {
        self.options.rules()
    }

    /// The compiled table for the stored rules, or `None` when there are none.
    pub fn table(&self) -> Result<Option<Rc<CompiledTable>>, RewriteError> {
        match self.rule_set()? {
            Some(rules) => self.table_for(rules).map(Some),
            None => Ok(None),
        }
    }

    fn table_for(&self, rules: Vec<Rule>) -> Result<Rc<CompiledTable>, RewriteError> {
        let mut router: Router<()> = Router::new();
        router.rules(RuleSlot::AfterRoutes, rules);
        let fingerprint = router
            .fingerprint()
            .map_err(|err| RewriteError(err.to_string()))?;
        self.cache
            .table(fingerprint, || router.data())
            .map_err(RewriteError)
    }

    /// Matches `subject` against the stored rules, as `parse_request` does.
    pub fn resolve(
        &self,
        subject: &WpSubject,
        guard: &dyn RuleGuard,
    ) -> Result<WpOutcome, RewriteError> {
        let Some(table) = self.table()? else {
            return Ok(WpOutcome::NoRewrite);
        };
        Ok(outcome(table.resolve(
            &request_for(subject),
            guard,
            &[],
            false,
        )))
    }

    /// Matches `subject` against `rules`: in PHP, the `$rewrite` array `parse_request` already holds.
    pub fn resolve_rules(
        &self,
        rules: Vec<Rule>,
        subject: &WpSubject,
        guard: &dyn RuleGuard,
    ) -> Result<WpOutcome, RewriteError> {
        if rules.is_empty() {
            return Ok(WpOutcome::NoRewrite);
        }
        let table = self.table_for(rules)?;
        Ok(outcome(table.resolve(
            &request_for(subject),
            guard,
            &[],
            false,
        )))
    }

    /// Continues from rule `from` after PHP rejected a pending guard.
    pub fn resume(
        &self,
        subject: &WpSubject,
        from: usize,
        guard: &dyn RuleGuard,
    ) -> Result<WpOutcome, RewriteError> {
        let Some(table) = self.table()? else {
            return Ok(WpOutcome::NoRewrite);
        };
        Ok(match table.resume(subject, from, guard) {
            Some(found) => outcome(Resolution::Route(found)),
            None => WpOutcome::NoMatch,
        })
    }
}

fn request_for(subject: &WpSubject) -> RouteRequest {
    RouteRequest {
        method: Method::Get,
        path: format!("/{}", subject.requested_path),
        wordpress: Some(subject.clone()),
    }
}

fn outcome(resolution: Resolution) -> WpOutcome {
    match resolution {
        Resolution::Route(RouteMatch::Rule(m)) => WpOutcome::Matched(m),
        Resolution::Route(RouteMatch::PendingGuard { rule_index }) => {
            WpOutcome::PendingGuard(rule_index)
        }
        Resolution::Route(RouteMatch::NeedsPhp { rule_index }) => WpOutcome::NeedsPhp(rule_index),
        _ => WpOutcome::NoMatch,
    }
}
