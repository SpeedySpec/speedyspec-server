mod support;

use speedyspec_routing::{EndpointMask, PermastructArgs, Rule, RulePriority};
use speedyspec_wp_rewrite::{EndpointQueryVar, RewriteState, RuleQuery};
use support::fixture;

fn state(f: &support::Fixture) -> RewriteState {
    let mut state = RewriteState::default();
    f.service.init(&mut state).unwrap();
    state
}

#[test]
fn add_rule_top_and_bottom() {
    let f = fixture("/%postname%/");
    let mut s = state(&f);
    f.service.add_rule(
        &mut s,
        "a/?$",
        RuleQuery::Text("index.php?a=1".into()),
        RulePriority::Bottom,
    );
    f.service.add_rule(
        &mut s,
        "b/?$",
        RuleQuery::Pairs(vec![
            ("a".into(), "1".into()),
            ("b".into(), "$matches[1]".into()),
        ]),
        RulePriority::Top,
    );
    assert_eq!(
        s.extra_rules,
        vec![Rule {
            pattern: "a/?$".into(),
            query: "index.php?a=1".into()
        }]
    );
    assert_eq!(
        s.extra_rules_top,
        vec![Rule {
            pattern: "b/?$".into(),
            query: "index.php?a=1&b=$matches[1]".into()
        }]
    );
}

#[test]
fn add_external_rule_is_non_wp() {
    let f = fixture("/%postname%/");
    let mut s = state(&f);
    f.service.add_rule(
        &mut s,
        "x/?$",
        RuleQuery::Text("other.php?x=1".into()),
        RulePriority::Bottom,
    );
    assert!(s.extra_rules.is_empty());
    assert_eq!(
        s.non_wp_rules,
        vec![Rule {
            pattern: "x/?$".into(),
            query: "other.php?x=1".into()
        }]
    );
}

#[test]
fn add_endpoint_registers_query_var_name() {
    let f = fixture("/%postname%/");
    let mut s = state(&f);
    f.service.add_endpoint(
        &mut s,
        "json",
        EndpointMask::ALL,
        EndpointQueryVar::SameAsName,
    );
    f.service
        .add_endpoint(&mut s, "quiet", EndpointMask::PAGES, EndpointQueryVar::None);
    f.service.add_endpoint(
        &mut s,
        "custom",
        EndpointMask::ROOT,
        EndpointQueryVar::Named("cv".into()),
    );
    assert_eq!(
        f.query_vars.added(),
        vec!["json".to_string(), "cv".to_string()]
    );
    assert_eq!(s.endpoints[1].query_var, "");
    assert_eq!(s.endpoints[2].query_var, "cv");
}

#[test]
fn rewrite_tags_replace_or_append() {
    let f = fixture("/%postname%/");
    let mut s = state(&f);
    let count = s.tags.len();
    f.service
        .add_rewrite_tag(&mut s, "%postname%", "(.+?)", "name=");
    assert_eq!(s.tags.len(), count);
    assert_eq!(
        s.tags.iter().find(|t| t.tag == "%postname%").unwrap().regex,
        "(.+?)"
    );
    f.service
        .add_rewrite_tag(&mut s, "%book%", "([^/]+)", "book=");
    assert_eq!(s.tags.len(), count + 1);
    f.service.remove_rewrite_tag(&mut s, "%book%");
    assert_eq!(s.tags.len(), count);
}

#[test]
fn permastructs_take_front_or_root() {
    let f = fixture("/blog/%postname%/");
    let mut s = state(&f);
    f.service
        .add_permastruct(&mut s, "genre", "genre/%genre%", PermastructArgs::default());
    let no_front = PermastructArgs {
        with_front: false,
        ..PermastructArgs::default()
    };
    f.service
        .add_permastruct(&mut s, "shelf", "shelf/%shelf%", no_front);
    assert_eq!(
        f.service.get_extra_permastruct(&s, "genre").as_deref(),
        Some("/blog/genre/%genre%")
    );
    assert_eq!(
        f.service.get_extra_permastruct(&s, "shelf").as_deref(),
        Some("shelf/%shelf%")
    );
    f.service.remove_permastruct(&mut s, "genre");
    assert_eq!(f.service.get_extra_permastruct(&s, "genre"), None);
}
