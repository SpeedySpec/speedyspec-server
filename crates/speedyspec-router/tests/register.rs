use speedyspec_http::Method;
use speedyspec_router::{CompileError, Group, Router, RuleSlot, Target};
use speedyspec_routing::{RouteId, Rule};

#[test]
fn verbs_register_methods() {
    let mut router: Router<&str> = Router::new();
    router.get("/g", "g");
    router.any("/any", "any");
    router.match_(&[Method::Put, Method::Delete], "/m", "m");
    let data = router.data().unwrap();
    let methods = |path: &str| -> Vec<Method> {
        data.statics
            .iter()
            .filter(|r| r.path == path)
            .map(|r| r.method.clone())
            .collect()
    };
    assert_eq!(methods("/g"), vec![Method::Get]);
    assert_eq!(methods("/any").len(), 7);
    assert_eq!(methods("/m"), vec![Method::Put, Method::Delete]);
}

#[test]
fn dynamic_routes_keep_registration_order() {
    let mut router: Router<&str> = Router::new();
    router.get("/a/{x}", "a");
    router.post("/b/{y}", "b");
    let data = router.data().unwrap();
    assert_eq!(
        data.dynamics.iter().map(|d| d.id).collect::<Vec<_>>(),
        vec![RouteId(0), RouteId(1)]
    );
    assert_eq!(data.dynamics[1].params, vec!["y".to_string()]);
}

#[test]
fn group_prefix_and_names_nest() {
    let mut router: Router<&str> = Router::new();
    router.group(Group::new().prefix("/api").name("api."), |r| {
        r.group(Group::new().prefix("/v1").name("v1."), |r| {
            r.post("/posts", "store").name("store");
        });
    });
    let data = router.data().unwrap();
    assert_eq!(data.statics[0].path, "/api/v1/posts");
    assert_eq!(data.names[0].0.as_str(), "api.v1.store");
}

#[test]
fn group_middleware_appends() {
    let mut router: Router<&str, &str> = Router::new();
    router.group(Group::new().middleware("outer"), |r| {
        r.group(Group::new().middleware("inner"), |r| {
            r.get("/x", "x").middleware("own");
        });
    });
    router.get("/y", "y");
    assert_eq!(
        router.middleware_for(RouteId(0)),
        &["outer", "inner", "own"]
    );
    assert!(router.middleware_for(RouteId(1)).is_empty());
}

#[test]
fn duplicate_name_is_an_error() {
    let mut router: Router<&str> = Router::new();
    router.get("/a", "a").name("x");
    router.get("/b", "b").name("x");
    assert!(matches!(router.data(), Err(CompileError::DuplicateName(_))));
}

#[test]
fn redirect_and_fallback_targets() {
    let mut router: Router<&str> = Router::new();
    router.redirect("/old", "/new", 301);
    router.fallback("missing");
    assert!(
        matches!(router.target(RouteId(0)), Target::Redirect { to, status: 301 } if to == "/new")
    );
    assert_eq!(
        router.redirects(),
        vec![(RouteId(0), "/new".to_string(), 301)]
    );
    assert_eq!(router.fallback_target(), Some(&"missing"));
}

#[test]
fn fingerprint_changes_with_rules() {
    let mut router: Router<&str> = Router::new();
    router.get("/a", "a");
    let before = router.fingerprint().unwrap();
    router.rules(
        RuleSlot::AfterRoutes,
        vec![Rule {
            pattern: "x/?$".into(),
            query: "index.php?x=1".into(),
        }],
    );
    let after = router.fingerprint().unwrap();
    assert_ne!(before, after);
    assert_eq!(router.data().unwrap().after_rules.len(), 1);
}
