use std::cell::RefCell;

use speedyspec_http::Method;
use speedyspec_router::dispatch::{ChunkedAlternation, RegexSetStrategy};
use speedyspec_router::{CompiledTable, RouteRequest, Router, RuleSlot, WpSubject};
use speedyspec_routing::{
    DispatchStrategy, GuardAnswer, QueryValue, Resolution, RouteMatch, Rule, RuleGuard,
};

struct Guard {
    answer: Box<dyn Fn(&Rule, &[Option<String>]) -> GuardAnswer>,
    seen: RefCell<Vec<String>>,
}

impl Guard {
    fn accept() -> Guard {
        Guard::new(|_, _| GuardAnswer::Accept)
    }

    fn new(answer: impl Fn(&Rule, &[Option<String>]) -> GuardAnswer + 'static) -> Guard {
        Guard {
            answer: Box::new(answer),
            seen: RefCell::new(Vec::new()),
        }
    }
}

impl RuleGuard for Guard {
    fn check(&self, rule: &Rule, captures: &[Option<String>]) -> GuardAnswer {
        self.seen.borrow_mut().push(rule.pattern.clone());
        (self.answer)(rule, captures)
    }
}

fn rules(pairs: &[(&str, &str)]) -> Vec<Rule> {
    pairs
        .iter()
        .map(|(p, q)| Rule {
            pattern: p.to_string(),
            query: q.to_string(),
        })
        .collect()
}

fn strategies() -> Vec<Box<dyn DispatchStrategy>> {
    vec![
        Box::new(ChunkedAlternation::default()),
        Box::new(RegexSetStrategy::default()),
    ]
}

fn resolve_with(
    strategy: &dyn DispatchStrategy,
    rule_list: Vec<Rule>,
    path: &str,
    file: &str,
    guard: &dyn RuleGuard,
) -> Resolution {
    let mut router: Router<&str> = Router::new();
    router.rules(RuleSlot::AfterRoutes, rule_list);
    let table = CompiledTable::compile(&router.data().unwrap(), strategy).unwrap();
    let request = RouteRequest {
        method: Method::Get,
        path: format!("/{path}"),
        wordpress: Some(WpSubject {
            requested_path: path.into(),
            requested_file: file.into(),
        }),
    };
    table.resolve(&request, guard, &[], false)
}

fn matched(resolution: &Resolution) -> Option<(&str, &str)> {
    match resolution {
        Resolution::Route(RouteMatch::Rule(m)) => {
            Some((m.matched_rule.as_str(), m.matched_query.as_str()))
        }
        _ => None,
    }
}

#[test]
fn empty_path_matches_only_dollar() {
    for s in strategies() {
        let list = rules(&[
            ("(.*)", "index.php?all=$matches[1]"),
            ("$", "index.php?home=1"),
        ]);
        let r = resolve_with(s.as_ref(), list, "", "", &Guard::accept());
        assert_eq!(matched(&r), Some(("$", "home=1")));
        let r = resolve_with(
            s.as_ref(),
            rules(&[("(.*)", "index.php?all=1")]),
            "",
            "",
            &Guard::accept(),
        );
        assert_eq!(r, Resolution::NotFound { allowed: vec![] });
    }
}

#[test]
fn rule_match_has_query_vars() {
    for s in strategies() {
        let list = rules(&[(
            "([^/]+)(?:/([0-9]+))?/?$",
            "index.php?name=$matches[1]&page=$matches[2]",
        )]);
        match resolve_with(s.as_ref(), list, "hello-world", "", &Guard::accept()) {
            Resolution::Route(RouteMatch::Rule(m)) => {
                assert_eq!(m.matched_rule, "([^/]+)(?:/([0-9]+))?/?$");
                assert_eq!(m.matched_query, "name=hello-world&page=");
                assert_eq!(
                    m.query_vars,
                    vec![
                        ("name".to_string(), QueryValue::Text("hello-world".into())),
                        ("page".to_string(), QueryValue::Text("".into())),
                    ]
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[test]
fn encoded_path_tries_raw_then_decoded_per_rule() {
    for s in strategies() {
        let list = rules(&[
            ("caf%C3%A9/?$", "index.php?a=1"),
            ("café/?$", "index.php?b=1"),
        ]);
        let r = resolve_with(s.as_ref(), list, "caf%C3%A9", "", &Guard::accept());
        assert_eq!(matched(&r), Some(("caf%C3%A9/?$", "a=1")));
        let list = rules(&[
            ("café/?$", "index.php?b=1"),
            ("caf%C3%A9/?$", "index.php?a=1"),
        ]);
        let r = resolve_with(s.as_ref(), list, "caf%C3%A9", "", &Guard::accept());
        assert_eq!(matched(&r), Some(("café/?$", "b=1")));
    }
}

#[test]
fn anchor_rule_prepends_requested_file() {
    for s in strategies() {
        let list = rules(&[
            ("nomatch$", "index.php?x=1"),
            (
                "index.php/([0-9]{4})/([0-9]{2})/?$",
                "index.php?year=$matches[1]&monthnum=$matches[2]",
            ),
        ]);
        let r = resolve_with(s.as_ref(), list, "2024/01", "index.php", &Guard::accept());
        assert_eq!(matched(&r).map(|m| m.1), Some("year=2024&monthnum=01"));
    }
}

#[test]
fn anchor_switch_persists_for_later_rules() {
    for s in strategies() {
        let list = rules(&[
            ("index.php/zzz$", "index.php?x=1"),
            ("index.php/(.+)$", "index.php?rest=$matches[1]"),
        ]);
        let r = resolve_with(s.as_ref(), list, "2024/01", "index.php", &Guard::accept());
        assert_eq!(matched(&r).map(|m| m.1), Some("rest=2024%2F01"));
    }
}

#[test]
fn rejected_guard_resumes_next_rule() {
    for s in strategies() {
        let list = rules(&[
            ("(.+?)/?$", "index.php?pagename=$matches[1]"),
            ("(.+?)/?$", "index.php?name=$matches[1]"),
        ]);
        let guard = Guard::new(|rule, _| {
            if rule.query.contains("pagename") {
                GuardAnswer::Reject
            } else {
                GuardAnswer::Accept
            }
        });
        let r = resolve_with(s.as_ref(), list, "missing-page", "", &guard);
        assert_eq!(matched(&r).map(|m| m.1), Some("name=missing-page"));
        assert_eq!(guard.seen.borrow().len(), 2);
    }
}

#[test]
fn unavailable_guard_is_pending() {
    for s in strategies() {
        let list = rules(&[
            ("x$", "index.php?x=1"),
            ("(.+?)/?$", "index.php?pagename=$matches[1]"),
        ]);
        let guard = Guard::new(|_, _| GuardAnswer::Unavailable);
        let r = resolve_with(s.as_ref(), list, "about", "", &guard);
        assert_eq!(
            r,
            Resolution::Route(RouteMatch::PendingGuard { rule_index: 1 })
        );
    }
}

#[test]
fn resume_from_pending_continues() {
    for s in strategies() {
        let list = rules(&[
            ("(.+?)/?$", "index.php?pagename=$matches[1]"),
            ("(.+?)/?$", "index.php?name=$matches[1]"),
        ]);
        let mut router: Router<&str> = Router::new();
        router.rules(RuleSlot::AfterRoutes, list);
        let table = CompiledTable::compile(&router.data().unwrap(), s.as_ref()).unwrap();
        let subject = WpSubject {
            requested_path: "about".into(),
            requested_file: "".into(),
        };
        match table.resume(&subject, 1, &Guard::accept()) {
            Some(RouteMatch::Rule(m)) => assert_eq!(m.matched_query, "name=about"),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(table.resume(&subject, 2, &Guard::accept()), None);
    }
}

#[test]
fn needs_php_stops_in_order() {
    for s in strategies() {
        let list = rules(&[
            ("x$", "index.php?x=1"),
            ("(?|(a)|(b))/?$", "index.php?p=$matches[1]"),
            ("b$", "index.php?b=1"),
        ]);
        let r = resolve_with(s.as_ref(), list.clone(), "b", "", &Guard::accept());
        assert_eq!(r, Resolution::Route(RouteMatch::NeedsPhp { rule_index: 1 }));
        let r = resolve_with(s.as_ref(), list, "x", "", &Guard::accept());
        assert_eq!(matched(&r), Some(("x$", "x=1")));
    }
}

#[test]
fn rules_skipped_without_wordpress_subject() {
    let mut router: Router<&str> = Router::new();
    router.rules(RuleSlot::AfterRoutes, rules(&[("(.*)", "index.php?all=1")]));
    let table =
        CompiledTable::compile(&router.data().unwrap(), &ChunkedAlternation::default()).unwrap();
    let request = RouteRequest {
        method: Method::Get,
        path: "/anything".into(),
        wordpress: None,
    };
    assert_eq!(
        table.resolve(&request, &Guard::accept(), &[], false),
        Resolution::NotFound { allowed: vec![] }
    );
}

#[test]
fn routes_win_over_after_rules_and_before_rules_win_over_routes() {
    let mut router: Router<&str> = Router::new();
    router.get("/sitemap.xml", "sitemap");
    router.rules(
        RuleSlot::AfterRoutes,
        rules(&[("sitemap\\.xml", "index.php?sitemap=index")]),
    );
    let table =
        CompiledTable::compile(&router.data().unwrap(), &ChunkedAlternation::default()).unwrap();
    let request = RouteRequest {
        method: Method::Get,
        path: "/sitemap.xml".into(),
        wordpress: Some(WpSubject {
            requested_path: "sitemap.xml".into(),
            requested_file: "sitemap.xml".into(),
        }),
    };
    assert!(matches!(
        table.resolve(&request, &Guard::accept(), &[], false),
        Resolution::Route(RouteMatch::Route { .. })
    ));
    router.rules(
        RuleSlot::BeforeRoutes,
        rules(&[("sitemap\\.xml", "index.php?before=1")]),
    );
    let table =
        CompiledTable::compile(&router.data().unwrap(), &ChunkedAlternation::default()).unwrap();
    assert_eq!(
        matched(&table.resolve(&request, &Guard::accept(), &[], false)).map(|m| m.1),
        Some("before=1")
    );
}
