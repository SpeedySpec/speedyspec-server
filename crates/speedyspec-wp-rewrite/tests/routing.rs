mod support;

use std::rc::Rc;

use speedyspec_router::dispatch::ChunkedAlternation;
use speedyspec_router::{TableCache, WpSubject};
use speedyspec_router_store_memory::MemoryRouteTableStore;
use speedyspec_routing::{GuardAnswer, Rule, RuleGuard};
use speedyspec_wp_rewrite::{RewriteOptions, RewriteState, WpOutcome, WpRouting};
use support::fixture;

struct Accept;

/// The verbose page guard for a site with no pages: every `pagename` rule is rejected.
struct NoPages;

impl RuleGuard for NoPages {
    fn check(&self, rule: &Rule, _captures: &[Option<String>]) -> GuardAnswer {
        if rule.query.contains("pagename=") {
            GuardAnswer::Reject
        } else {
            GuardAnswer::Accept
        }
    }
}

impl RuleGuard for Accept {
    fn check(&self, _rule: &Rule, _captures: &[Option<String>]) -> GuardAnswer {
        GuardAnswer::Accept
    }
}

fn routing(f: &support::Fixture) -> WpRouting {
    let cache = Rc::new(TableCache::new(
        Rc::new(MemoryRouteTableStore::new()),
        Rc::new(ChunkedAlternation::default()),
    ));
    WpRouting::new(f.options.clone(), cache)
}

fn subject(path: &str) -> WpSubject {
    WpSubject {
        requested_path: path.into(),
        requested_file: path.into(),
    }
}

#[test]
fn routing_matches_postname() {
    let f = fixture("/%postname%/");
    let mut s = RewriteState::default();
    f.service.init(&mut s).unwrap();
    f.service
        .wp_rewrite_rules(&mut s, f.source.as_ref())
        .unwrap();
    let routing = routing(&f);
    match routing.resolve(&subject("hello-world"), &NoPages).unwrap() {
        WpOutcome::Matched(m) => {
            assert_eq!(m.matched_rule, "([^/]+)(?:/([0-9]+))?/?$");
            assert_eq!(m.matched_query, "name=hello-world&page=");
        }
        other => panic!("unexpected {other:?}"),
    }
    match routing.resolve(&subject("about"), &Accept).unwrap() {
        WpOutcome::Matched(m) => assert_eq!(m.matched_query, "pagename=about&page="),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn plain_permalinks_are_no_rewrite() {
    let f = fixture("");
    assert_eq!(
        routing(&f).resolve(&subject("anything"), &Accept).unwrap(),
        WpOutcome::NoRewrite
    );
}

#[test]
fn unmatched_path_is_no_match() {
    let f = fixture("/%postname%/");
    f.options
        .set_rules(&[Rule {
            pattern: "only$".into(),
            query: "index.php?only=1".into(),
        }])
        .unwrap();
    assert_eq!(
        routing(&f)
            .resolve(&subject("else/where"), &Accept)
            .unwrap(),
        WpOutcome::NoMatch
    );
}

#[test]
fn routing_rebuilds_after_option_change() {
    let f = fixture("/%postname%/");
    let routing = routing(&f);
    f.options
        .set_rules(&[Rule {
            pattern: "a$".into(),
            query: "index.php?a=1".into(),
        }])
        .unwrap();
    assert!(matches!(
        routing.resolve(&subject("a"), &Accept).unwrap(),
        WpOutcome::Matched(_)
    ));
    f.options
        .set_rules(&[Rule {
            pattern: "b$".into(),
            query: "index.php?b=1".into(),
        }])
        .unwrap();
    assert_eq!(
        routing.resolve(&subject("a"), &Accept).unwrap(),
        WpOutcome::NoMatch
    );
    assert!(matches!(
        routing.resolve(&subject("b"), &Accept).unwrap(),
        WpOutcome::Matched(_)
    ));
}
