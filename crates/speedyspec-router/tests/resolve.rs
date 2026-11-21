use speedyspec_http::Method;
use speedyspec_router::dispatch::ChunkedAlternation;
use speedyspec_router::{CompiledTable, RouteRequest, Router};
use speedyspec_routing::{GuardAnswer, Resolution, RouteId, RouteMatch, Rule, RuleGuard};

struct AcceptAll;

impl RuleGuard for AcceptAll {
    fn check(&self, _rule: &Rule, _captures: &[Option<String>]) -> GuardAnswer {
        GuardAnswer::Accept
    }
}

fn resolve(router: &Router<&str>, method: Method, path: &str) -> Resolution {
    let table =
        CompiledTable::compile(&router.data().unwrap(), &ChunkedAlternation::default()).unwrap();
    let request = RouteRequest {
        method,
        path: path.to_string(),
        wordpress: None,
    };
    table.resolve(
        &request,
        &AcceptAll,
        &router.redirects(),
        router.fallback_target().is_some(),
    )
}

fn route_id(resolution: &Resolution) -> Option<RouteId> {
    match resolution {
        Resolution::Route(RouteMatch::Route { id, .. }) => Some(*id),
        _ => None,
    }
}

#[test]
fn static_beats_dynamic() {
    let mut router: Router<&str> = Router::new();
    router.get("/users/{id}", "show");
    router.get("/users/me", "me");
    assert_eq!(
        route_id(&resolve(&router, Method::Get, "/users/me")),
        Some(RouteId(1))
    );
}

#[test]
fn dynamic_in_registration_order_with_params() {
    let mut router: Router<&str> = Router::new();
    router.get("/a/{x}", "first").name("first");
    router.get("/a/{y}", "second");
    match resolve(&router, Method::Get, "/a/42") {
        Resolution::Route(RouteMatch::Route { id, name, params }) => {
            assert_eq!(id, RouteId(0));
            assert_eq!(name.unwrap().as_str(), "first");
            assert_eq!(params.get("x"), Some("42"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn head_falls_back_to_get() {
    let mut router: Router<&str> = Router::new();
    router.get("/a", "a");
    router.get("/b/{x}", "b");
    assert_eq!(
        route_id(&resolve(&router, Method::Head, "/a")),
        Some(RouteId(0))
    );
    assert_eq!(
        route_id(&resolve(&router, Method::Head, "/b/1")),
        Some(RouteId(1))
    );
}

#[test]
fn wrong_method_gives_allowed() {
    let mut router: Router<&str> = Router::new();
    router.get("/users/{id}", "show");
    router.get("/static", "s");
    assert_eq!(
        resolve(&router, Method::Post, "/users/1"),
        Resolution::NotFound {
            allowed: vec![Method::Get, Method::Head]
        }
    );
    assert_eq!(
        resolve(&router, Method::Delete, "/static"),
        Resolution::NotFound {
            allowed: vec![Method::Get, Method::Head]
        }
    );
}

#[test]
fn method_mismatch_keeps_looking() {
    let mut router: Router<&str> = Router::new();
    router.get("/x/{a}", "get");
    router.post("/x/{b}", "post");
    assert_eq!(
        route_id(&resolve(&router, Method::Post, "/x/1")),
        Some(RouteId(1))
    );
}

#[test]
fn redirect_route_resolves() {
    let mut router: Router<&str> = Router::new();
    router.redirect("/old", "/new", 301);
    assert_eq!(
        resolve(&router, Method::Get, "/old"),
        Resolution::Redirect {
            location: "/new".into(),
            status: 301
        }
    );
}

#[test]
fn fallback_when_nothing_matches() {
    let mut router: Router<&str> = Router::new();
    router.get("/a", "a");
    assert_eq!(
        resolve(&router, Method::Get, "/zzz"),
        Resolution::NotFound { allowed: vec![] }
    );
    router.fallback("missing");
    assert_eq!(
        route_id(&resolve(&router, Method::Get, "/zzz")),
        Some(RouteId::FALLBACK)
    );
}
