//! The REST route table: one table for every route, matched as `WP_REST_Server::match_request_to_handler` does.

use indexmap::IndexMap;
use regex::Regex;
use speedyspec_operation_contracts::Value;
use speedyspec_rest_contracts::{RestBinding, RestHooks, RestMethod};
use speedyspec_transport_contracts::{BindingSource, ExposureLookup};

use crate::transport::REST;

/// One route of the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteEntry {
    /// The full route pattern, `/{namespace}{route}`.
    pub route: String,
    pub namespace: String,
    pub methods: Vec<RestMethod>,
    pub operation: String,
}

/// What matching a request found.
#[derive(Clone, Debug, PartialEq)]
pub enum RouteMatch {
    Found {
        operation: String,
        route: String,
        url_params: IndexMap<String, Value>,
    },
    NoRoute,
}

struct Compiled {
    entry: RouteEntry,
    pattern: Regex,
}

/// Every REST route, in the order added: explicit bindings first, then defaults.
pub struct RouteTable {
    routes: Vec<Compiled>,
}

impl RouteTable {
    /// The table for what `exposure` exposes on `rest`, with `rest_endpoints` applied. Collisions are skipped and
    /// returned as problems: an explicit binding wins over a default; the first explicit binding wins over a later one.
    pub fn build(
        exposure: &dyn ExposureLookup,
        hooks: &dyn RestHooks,
    ) -> (RouteTable, Vec<String>) {
        let exposed = exposure.exposed(REST);
        let ordered = exposed
            .iter()
            .filter(|e| e.source == BindingSource::Explicit)
            .chain(
                exposed
                    .iter()
                    .filter(|e| e.source == BindingSource::Default),
            );
        let mut routes: Vec<Compiled> = Vec::new();
        let mut problems = Vec::new();
        for exposed in ordered {
            let operation = exposed.spec.name.as_str().to_owned();
            let Ok(binding) = RestBinding::from_value(&exposed.binding) else {
                continue;
            };
            let full = binding.full_route();
            let mut methods = Vec::new();
            for method in &binding.methods {
                let taken = routes.iter().find(|c| {
                    c.entry.route.eq_ignore_ascii_case(&full) && c.entry.methods.contains(method)
                });
                match taken {
                    Some(other) if exposed.source == BindingSource::Default => problems.push(format!(
                        "{operation}: the default route {full} {} is taken by {}; bind it explicitly",
                        method.as_str(),
                        other.entry.operation
                    )),
                    Some(other) => problems.push(format!(
                        "{operation}: {full} {} is already bound to {}",
                        method.as_str(),
                        other.entry.operation
                    )),
                    None => methods.push(*method),
                }
            }
            if methods.is_empty() {
                continue;
            }
            let Ok(pattern) = Regex::new(&format!("(?i)^{full}$")) else {
                problems.push(format!("{operation}: {full} is not a valid route pattern"));
                continue;
            };
            routes.push(Compiled {
                entry: RouteEntry {
                    route: full,
                    namespace: binding.namespace.clone(),
                    methods,
                    operation,
                },
                pattern,
            });
        }
        let mut patterns: Vec<String> = Vec::new();
        for c in &routes {
            if !patterns.contains(&c.entry.route) {
                patterns.push(c.entry.route.clone());
            }
        }
        let kept = hooks.endpoints(patterns);
        routes.retain(|c| kept.contains(&c.entry.route));
        (RouteTable { routes }, problems)
    }

    /// The route serving `method` on `path`. `HEAD` with no `HEAD` route falls back to the `GET` route, as WordPress
    /// does. A path that matches with another method is still `NoRoute`, as WordPress answers `rest_no_route`.
    pub fn find(&self, method: RestMethod, path: &str) -> RouteMatch {
        match self.find_exact(method, path) {
            RouteMatch::NoRoute if method == RestMethod::Head => {
                self.find_exact(RestMethod::Get, path)
            }
            found => found,
        }
    }

    fn find_exact(&self, method: RestMethod, path: &str) -> RouteMatch {
        for c in &self.routes {
            if !c.entry.methods.contains(&method) {
                continue;
            }
            let Some(captures) = c.pattern.captures(path) else {
                continue;
            };
            let url_params = c
                .pattern
                .capture_names()
                .flatten()
                .filter_map(|name| {
                    captures
                        .name(name)
                        .map(|m| (name.to_owned(), Value::from(m.as_str())))
                })
                .collect();
            return RouteMatch::Found {
                operation: c.entry.operation.clone(),
                route: c.entry.route.clone(),
                url_params,
            };
        }
        RouteMatch::NoRoute
    }

    /// Every route, in table order.
    pub fn routes(&self) -> Vec<RouteEntry> {
        self.routes.iter().map(|c| c.entry.clone()).collect()
    }
}
