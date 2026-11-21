use std::collections::HashMap;

use speedyspec_http::Method;
use speedyspec_routing::{
    CompiledDispatch, DispatchStrategy, Params, Resolution, RouteData, RouteId, RouteMatch,
    RouteName, RuleGuard,
};

use crate::wordpress::RuleSetMatcher;

/// What WordPress's rules match against: the requested path and file, as `WP::parse_request` computes them.
///
/// Present only when the server sent the request to the app's entry script, since only `index.php` runs
/// `parse_request` in WordPress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WpSubject {
    pub requested_path: String,
    pub requested_file: String,
}

/// The part of a request the router resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteRequest {
    pub method: Method,
    pub path: String,
    pub wordpress: Option<WpSubject>,
}

struct Dynamic {
    methods: Vec<Method>,
    params: Vec<String>,
    id: RouteId,
}

/// A route table compiled from route data: immutable, built once per fingerprint.
pub struct CompiledTable {
    statics: HashMap<String, Vec<(Method, RouteId)>>,
    dynamics: Vec<Dynamic>,
    dispatch: Box<dyn CompiledDispatch>,
    names: HashMap<RouteId, RouteName>,
    before_rules: RuleSetMatcher,
    after_rules: RuleSetMatcher,
}

/// True when a route registered for `methods` answers `method` (`HEAD` falls back to `GET`).
fn answers(methods: &[Method], method: &Method) -> bool {
    methods.contains(method) || (*method == Method::Head && methods.contains(&Method::Get))
}

fn allowed_by(methods: &[Method], allowed: &mut Vec<Method>) {
    for method in methods {
        if !allowed.contains(method) {
            allowed.push(method.clone());
        }
        if *method == Method::Get
            && !allowed.contains(&Method::Head)
            && !methods.contains(&Method::Head)
        {
            allowed.push(Method::Head);
        }
    }
}

impl CompiledTable {
    pub fn compile(
        data: &RouteData,
        strategy: &dyn DispatchStrategy,
    ) -> Result<CompiledTable, String> {
        let mut statics: HashMap<String, Vec<(Method, RouteId)>> = HashMap::new();
        for route in &data.statics {
            statics
                .entry(route.path.clone())
                .or_default()
                .push((route.method.clone(), route.id));
        }
        let patterns: Vec<String> = data.dynamics.iter().map(|d| d.regex.clone()).collect();
        let dynamics = data
            .dynamics
            .iter()
            .map(|d| Dynamic {
                methods: d.methods.clone(),
                params: d.params.clone(),
                id: d.id,
            })
            .collect();
        Ok(CompiledTable {
            statics,
            dynamics,
            dispatch: strategy.build(&patterns)?,
            names: data
                .names
                .iter()
                .map(|(name, id)| (*id, name.clone()))
                .collect(),
            before_rules: RuleSetMatcher::compile(&data.before_rules, strategy)?,
            after_rules: RuleSetMatcher::compile(&data.after_rules, strategy)?,
        })
    }

    fn route(&self, id: RouteId, params: Params) -> Resolution {
        Resolution::Route(RouteMatch::Route {
            id,
            name: self.names.get(&id).cloned(),
            params,
        })
    }

    /// Continues matching WordPress's rules (the after-routes slot) from rule `from`, after a guard PHP answered
    /// rejected the rule before it.
    pub fn resume(
        &self,
        subject: &WpSubject,
        from: usize,
        guard: &dyn RuleGuard,
    ) -> Option<RouteMatch> {
        self.after_rules.find(subject, from, guard)
    }

    /// Resolves a request: rules before routes, static routes, dynamic routes in registration order, rules after
    /// routes, then the fallback.
    ///
    /// `redirects` names the routes that are redirects, which resolve to [`Resolution::Redirect`].
    pub fn resolve(
        &self,
        request: &RouteRequest,
        guard: &dyn RuleGuard,
        redirects: &[(RouteId, String, u16)],
        has_fallback: bool,
    ) -> Resolution {
        let redirect_or_route =
            |id: RouteId, params: Params| match redirects.iter().find(|(rid, _, _)| *rid == id) {
                Some((_, location, status)) => Resolution::Redirect {
                    location: location.clone(),
                    status: *status,
                },
                None => self.route(id, params),
            };
        let mut allowed = Vec::new();

        if let Some(subject) = &request.wordpress
            && let Some(found) = self.before_rules.find(subject, 0, guard)
        {
            return Resolution::Route(found);
        }

        if let Some(routes) = self.statics.get(&request.path) {
            let exact = routes.iter().find(|(method, _)| *method == request.method);
            let head = routes
                .iter()
                .find(|(method, _)| request.method == Method::Head && *method == Method::Get);
            if let Some((_, id)) = exact.or(head) {
                return redirect_or_route(*id, Params::default());
            }
            let methods: Vec<Method> = routes.iter().map(|(method, _)| method.clone()).collect();
            allowed_by(&methods, &mut allowed);
        }

        let mut from = 0;
        while let Some((index, captures)) = self.dispatch.find(request.path.as_bytes(), from) {
            let dynamic = &self.dynamics[index];
            if answers(&dynamic.methods, &request.method) {
                let params = dynamic
                    .params
                    .iter()
                    .zip(captures.iter().skip(1))
                    // Decoded as Laravel hands parameters out; matching ran on the raw path, so an encoded `/`
                    // stays inside its segment.
                    .filter_map(|(name, value)| {
                        value
                            .as_ref()
                            .map(|value| (name.clone(), crate::wordpress::php_rawurldecode(value)))
                    })
                    .collect();
                return redirect_or_route(dynamic.id, Params(params));
            }
            allowed_by(&dynamic.methods, &mut allowed);
            from = index + 1;
        }

        if let Some(subject) = &request.wordpress
            && let Some(found) = self.after_rules.find(subject, 0, guard)
        {
            return Resolution::Route(found);
        }

        if has_fallback && allowed.is_empty() {
            return self.route(RouteId::FALLBACK, Params::default());
        }
        Resolution::NotFound { allowed }
    }
}
