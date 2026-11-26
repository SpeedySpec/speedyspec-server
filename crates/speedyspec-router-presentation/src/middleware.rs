use std::rc::Rc;

use speedyspec_http::{
    Handler, Message, Method, Middleware, Request, ServerRequest, StatusCode, Uri,
};
use speedyspec_http_message::{HttpResponse, HttpServerRequest};
use speedyspec_http_pipeline::Pipeline;
use speedyspec_router::wordpress::wp_sanitize_redirect;
use speedyspec_router::{RouteRequest, Router, TableCache, Target};
use speedyspec_routing::{GuardAnswer, Resolution, RouteId, RouteMatch, Rule, RuleGuard};
use speedyspec_wp_hooks::WpHookApi;
use speedyspec_wp_hooks_contracts::{HookName, HookValue};
use speedyspec_wp_rewrite::{WpOutcome, WpRouting};

use crate::{RedirectSources, wp_subject};

/// A native route's handler.
pub type AppHandler = Rc<dyn Handler<HttpServerRequest, HttpResponse>>;

/// A native route's middleware.
pub type AppMiddleware = Rc<dyn Middleware<HttpServerRequest, HttpResponse>>;

/// Which hooks have a PHP callback (the registry the runtime builds when a worker starts). A native path that would
/// fire such a hook hands the request to PHP instead.
pub trait PhpCallbackRegistry {
    fn has_php_callback(&self, hook: &str) -> bool;
}

/// The verbose page check before PHP starts: pages are not ported, so a rule that names a page cannot be decided
/// here and PHP finishes from it; every other rule is accepted.
pub struct UnansweredPageGuard;

impl RuleGuard for UnansweredPageGuard {
    fn check(&self, rule: &Rule, _captures: &[Option<String>]) -> GuardAnswer {
        if rule.query.contains("pagename=$matches[") {
            GuardAnswer::Unavailable
        } else {
            GuardAnswer::Accept
        }
    }
}

/// Request stage 3: native routes, redirects, and WordPress's rules (stored for PHP).
pub struct RoutingMiddleware {
    router: Rc<Router<AppHandler, AppMiddleware>>,
    cache: Rc<TableCache>,
    routing: Rc<WpRouting>,
    sources: RedirectSources,
    hooks: Rc<WpHookApi>,
    registry: Rc<dyn PhpCallbackRegistry>,
    guard: Rc<dyn RuleGuard>,
    home_path: String,
    index: String,
}

impl RoutingMiddleware {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        router: Rc<Router<AppHandler, AppMiddleware>>,
        cache: Rc<TableCache>,
        routing: Rc<WpRouting>,
        sources: RedirectSources,
        hooks: Rc<WpHookApi>,
        registry: Rc<dyn PhpCallbackRegistry>,
        guard: Rc<dyn RuleGuard>,
        home_path: &str,
        index: &str,
    ) -> RoutingMiddleware {
        RoutingMiddleware {
            router,
            cache,
            routing,
            sources,
            hooks,
            registry,
            guard,
            home_path: home_path.to_string(),
            index: index.to_string(),
        }
    }

    /// Answers a redirect as `wp_redirect()` would: the `wp_redirect` and `wp_redirect_status` filters decide the
    /// location and status, a falsy location sends nothing, the location is sanitized (`wp_sanitize_redirect`), and
    /// the `x_redirect_by` filter decides the `X-Redirect-By` header. `None` means PHP must decide: a PHP callback is
    /// on one of the filters, a filter failed or returned what only PHP can read, or the status is outside 3xx (where
    /// `wp_redirect()` calls `wp_die()`).
    fn redirect(&self, location: String, status: u16) -> Option<Option<HttpResponse>> {
        if ["wp_redirect", "wp_redirect_status", "x_redirect_by"]
            .iter()
            .any(|hook| self.registry.has_php_callback(hook))
        {
            return None;
        }
        let location = self
            .hooks
            .apply_filters(
                &HookName::from("wp_redirect"),
                HookValue::Str(location),
                vec![HookValue::Int(status.into())],
            )
            .ok()?
            .read()
            .into_owned();
        let status = self
            .hooks
            .apply_filters(
                &HookName::from("wp_redirect_status"),
                HookValue::Int(status.into()),
                vec![location.clone()],
            )
            .ok()?
            .read()
            .into_owned();
        let location = match location {
            HookValue::Str(s) if s.is_empty() || s == "0" => return Some(None),
            HookValue::Null | HookValue::Bool(false) => return Some(None),
            HookValue::Str(s) => s,
            _ => return None,
        };
        let status = match status {
            HookValue::Int(i) => i,
            HookValue::Str(s) => s.trim().parse().ok()?,
            _ => return None,
        };
        if !(300..=399).contains(&status) {
            return None;
        }
        let status = StatusCode::new(u16::try_from(status).ok()?).ok()?;
        let location = wp_sanitize_redirect(&location);
        let redirect_by = self
            .hooks
            .apply_filters(
                &HookName::from("x_redirect_by"),
                HookValue::Str("WordPress".into()),
                vec![
                    HookValue::Int(status.as_u16().into()),
                    HookValue::Str(location.clone()),
                ],
            )
            .ok()?
            .read()
            .into_owned();
        let mut response = HttpResponse::new(status);
        match redirect_by {
            HookValue::Str(by) => {
                response = response.with_header("X-Redirect-By", vec![by]).ok()?
            }
            HookValue::Bytes(_) => return None,
            _ => {}
        }
        Some(Some(response.with_header("Location", vec![location]).ok()?))
    }

    fn run_route(&self, id: RouteId, request: HttpServerRequest) -> HttpResponse {
        let Target::Handler(handler) = self.router.target(id) else {
            unreachable!("redirect routes resolve to Resolution::Redirect")
        };
        let mut pipeline = Pipeline::new(handler.clone());
        for middleware in self.router.middleware_for(id) {
            pipeline.pipe(middleware.clone());
        }
        pipeline.handle(request)
    }
}

fn method_not_allowed(allowed: &[Method]) -> HttpResponse {
    let allow = allowed
        .iter()
        .map(Method::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let response = HttpResponse::new(StatusCode::new(405).expect("405 is valid"));
    response
        .clone()
        .with_header("Allow", vec![allow])
        .unwrap_or(response)
}

impl Middleware<HttpServerRequest, HttpResponse> for RoutingMiddleware {
    fn process(
        &self,
        request: HttpServerRequest,
        next: &dyn Handler<HttpServerRequest, HttpResponse>,
    ) -> HttpResponse {
        let method = request.method().clone();
        let path = request.uri().path().to_string();
        let routes = match self.router.fingerprint() {
            Ok(fingerprint) => self.cache.table(fingerprint, || self.router.data()).ok(),
            Err(_) => None,
        };
        let mut allowed = Vec::new();
        if let Some(table) = &routes {
            let route_request = RouteRequest {
                method: method.clone(),
                path: path.clone(),
                wordpress: None,
            };
            match table.resolve(
                &route_request,
                self.guard.as_ref(),
                &self.router.redirects(),
                false,
            ) {
                Resolution::Route(RouteMatch::Route { id, name, params }) => {
                    let resolution = Resolution::Route(RouteMatch::Route { id, name, params });
                    return self.run_route(id, request.with_attribute(resolution));
                }
                Resolution::Redirect { location, status } => {
                    match self.redirect(location, status) {
                        Some(Some(response)) => return response,
                        Some(None) => return next.handle(request),
                        None => return next.handle(request),
                    }
                }
                Resolution::NotFound { allowed: methods } => allowed = methods,
                Resolution::Route(_) => {}
            }
        }

        if let Some((location, status)) = self.sources.redirect_for(&request)
            && let Some(Some(response)) = self.redirect(location, status)
        {
            return response;
        }

        if let Some(subject) = wp_subject(&request, &self.home_path, &self.index) {
            let resolution = match self.routing.resolve(&subject, self.guard.as_ref()) {
                Ok(WpOutcome::Matched(m)) => Resolution::Route(RouteMatch::Rule(m)),
                Ok(WpOutcome::PendingGuard(rule_index)) => {
                    Resolution::Route(RouteMatch::PendingGuard { rule_index })
                }
                Ok(WpOutcome::NeedsPhp(rule_index)) => {
                    Resolution::Route(RouteMatch::NeedsPhp { rule_index })
                }
                Ok(WpOutcome::NoMatch) | Ok(WpOutcome::NoRewrite) | Err(_) => {
                    Resolution::NotFound { allowed: vec![] }
                }
            };
            return next.handle(request.with_attribute(resolution));
        }

        if !allowed.is_empty() {
            return method_not_allowed(&allowed);
        }
        if let Some(fallback) = self.router.fallback_target() {
            return fallback
                .handle(request.with_attribute(Resolution::NotFound { allowed: vec![] }));
        }
        next.handle(request)
    }
}
