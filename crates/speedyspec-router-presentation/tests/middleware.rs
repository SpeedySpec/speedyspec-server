use std::collections::HashSet;
use std::rc::Rc;

use speedyspec_http::{
    Handler, Message, Method, Middleware, Response, ServerRequest, ServerRequestFactory, StatusCode,
};
use speedyspec_http_message::{HttpResponse, HttpServerRequest, MessageFactory};
use speedyspec_router::dispatch::ChunkedAlternation;
use speedyspec_router::{Router, TableCache};
use speedyspec_router_presentation::{
    AppHandler, AppMiddleware, PhpCallbackRegistry, RedirectSources, RoutingMiddleware,
    UnansweredPageGuard,
};
use speedyspec_router_store_memory::MemoryRouteTableStore;
use speedyspec_routing::{Resolution, RouteMatch, Rule};
use speedyspec_wp_hooks::{Callback, HookApi, HookName, HookValue, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_rewrite::memory::MemoryRewriteOptions;
use speedyspec_wp_rewrite::{RewriteOptions, WpRouting};

struct Respond(u16);

impl Handler<HttpServerRequest, HttpResponse> for Respond {
    fn handle(&self, _request: HttpServerRequest) -> HttpResponse {
        HttpResponse::new(StatusCode::new(self.0).unwrap())
    }
}

/// PHP: answers 200 with the resolution it was handed, as a header.
struct Php;

impl Handler<HttpServerRequest, HttpResponse> for Php {
    fn handle(&self, request: HttpServerRequest) -> HttpResponse {
        let seen = match request.attributes().get::<Resolution>() {
            Some(Resolution::Route(RouteMatch::Rule(m))) => m.matched_query.clone(),
            Some(other) => format!("{other:?}"),
            None => "none".into(),
        };
        HttpResponse::new(StatusCode::new(200).unwrap())
            .with_header("X-Seen", vec![seen])
            .unwrap()
    }
}

struct Registry(HashSet<&'static str>);

impl PhpCallbackRegistry for Registry {
    fn has_php_callback(&self, hook: &str) -> bool {
        self.0.contains(hook)
    }
}

fn cache() -> Rc<TableCache> {
    Rc::new(TableCache::new(
        Rc::new(MemoryRouteTableStore::new()),
        Rc::new(ChunkedAlternation::default()),
    ))
}

fn middleware(
    router: Router<AppHandler, AppMiddleware>,
    hooks: Rc<HookApi>,
    php_hooks: &[&'static str],
) -> RoutingMiddleware {
    let options = Rc::new(MemoryRewriteOptions::new());
    options
        .set_rules(&[Rule {
            pattern: "([^/]+)/?$".into(),
            query: "index.php?name=$matches[1]".into(),
        }])
        .unwrap();
    RoutingMiddleware::new(
        Rc::new(router),
        cache(),
        Rc::new(WpRouting::new(options, cache())),
        RedirectSources::new(vec![]),
        hooks,
        Rc::new(Registry(php_hooks.iter().copied().collect())),
        Rc::new(UnansweredPageGuard),
        "",
        "index.php",
    )
}

fn hooks() -> Rc<HookApi> {
    Rc::new(HookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ))
}

fn request(uri: &str, script: &str) -> HttpServerRequest {
    let path = uri.split('?').next().unwrap().to_string();
    let params = vec![
        ("SCRIPT_NAME".to_string(), script.to_string()),
        ("PHP_SELF".to_string(), script.to_string()),
        ("REQUEST_URI".to_string(), path),
    ];
    MessageFactory
        .create_server_request(Method::Get, &format!("http://example.test{uri}"), params)
        .unwrap()
}

#[test]
fn native_route_answers() {
    let mut router: Router<AppHandler, AppMiddleware> = Router::new();
    router.get("/robots.txt", Rc::new(Respond(204)));
    let response =
        middleware(router, hooks(), &[]).process(request("/robots.txt", "/index.php"), &Php);
    assert_eq!(response.status().as_u16(), 204);
}

#[test]
fn wordpress_match_passes_on_with_attribute() {
    let router: Router<AppHandler, AppMiddleware> = Router::new();
    let response =
        middleware(router, hooks(), &[]).process(request("/hello-world/", "/index.php"), &Php);
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        response.headers().get("x-seen").as_deref(),
        Some("name=hello-world")
    );
}

#[test]
fn route_method_mismatch_gives_405_with_allow() {
    let mut router: Router<AppHandler, AppMiddleware> = Router::new();
    router.post("/api/thing", Rc::new(Respond(201)));
    let response =
        middleware(router, hooks(), &[]).process(request("/api/thing", "/api.php"), &Php);
    assert_eq!(response.status().as_u16(), 405);
    assert_eq!(response.headers().get("allow").as_deref(), Some("POST"));
}

#[test]
fn redirect_fires_wp_redirect_filters() {
    let hooks = hooks();
    hooks
        .add_filter(
            &HookName::from("wp_redirect"),
            Callback::rust("rewrite_location", |_: &[HookValue]| {
                Ok(HookValue::Str("/changed".into()))
            }),
            &HookValue::Int(10),
            2,
        )
        .unwrap();
    let mut router: Router<AppHandler, AppMiddleware> = Router::new();
    router.redirect("/old", "/new", 301);
    let response = middleware(router, hooks, &[]).process(request("/old", "/index.php"), &Php);
    assert_eq!(response.status().as_u16(), 301);
    assert_eq!(
        response.headers().get("location").as_deref(),
        Some("/changed")
    );
}

#[test]
fn php_callback_on_wp_redirect_passes_on() {
    let mut router: Router<AppHandler, AppMiddleware> = Router::new();
    router.redirect("/old", "/new", 301);
    let response =
        middleware(router, hooks(), &["wp_redirect"]).process(request("/old", "/index.php"), &Php);
    assert_eq!(response.status().as_u16(), 200);
}

#[test]
fn route_middleware_wraps_the_handler() {
    struct Stamp;
    impl Middleware<HttpServerRequest, HttpResponse> for Stamp {
        fn process(
            &self,
            request: HttpServerRequest,
            next: &dyn Handler<HttpServerRequest, HttpResponse>,
        ) -> HttpResponse {
            next.handle(request)
                .with_header("X-Stamp", vec!["1".into()])
                .unwrap()
        }
    }
    let mut router: Router<AppHandler, AppMiddleware> = Router::new();
    router
        .get("/stamped", Rc::new(Respond(200)))
        .middleware(Rc::new(Stamp));
    let response =
        middleware(router, hooks(), &[]).process(request("/stamped", "/index.php"), &Php);
    assert_eq!(response.headers().get("x-stamp").as_deref(), Some("1"));
}
