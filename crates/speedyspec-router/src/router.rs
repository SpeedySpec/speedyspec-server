use std::fmt;

use sha2::{Digest, Sha256};
use speedyspec_http::Method;
use speedyspec_routing::{
    DynamicRoute, Fingerprint, RouteData, RouteId, RouteName, Rule, StaticRoute,
};

use crate::Group;
use crate::pattern::{PatternError, parse};

/// What a route leads to: a handler, or a redirect the router answers itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target<H> {
    Handler(H),
    Redirect { to: String, status: u16 },
}

/// Where a rule set sits relative to the registered routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleSlot {
    BeforeRoutes,
    AfterRoutes,
}

/// Why the routes could not be compiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    Pattern(PatternError),
    DuplicateName(String),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::Pattern(err) => write!(f, "{err}"),
            CompileError::DuplicateName(name) => write!(f, "two routes are named {name:?}"),
        }
    }
}

impl std::error::Error for CompileError {}

pub(crate) struct Entry<H, M> {
    pub(crate) methods: Vec<Method>,
    pub(crate) path: String,
    pub(crate) wheres: Vec<(String, String)>,
    pub(crate) name_prefix: String,
    pub(crate) name: Option<String>,
    pub(crate) target: Target<H>,
    pub(crate) middleware: Vec<M>,
}

impl<H, M> Entry<H, M> {
    pub(crate) fn full_name(&self) -> Option<String> {
        self.name
            .as_ref()
            .map(|name| format!("{}{name}", self.name_prefix))
    }
}

/// Every method a route registered with `any` answers.
const ALL_METHODS: [Method; 7] = [
    Method::Get,
    Method::Head,
    Method::Post,
    Method::Put,
    Method::Patch,
    Method::Delete,
    Method::Options,
];

/// Routes registered the way Laravel registers them, compiled into plain route data (Builder), with URL
/// generation; one object over the collector, compiler and generator (Facade).
///
/// `H` is the handler type and `M` the middleware type: the router stores them and never calls them, so it works
/// for any request and response types.
pub struct Router<H, M = ()> {
    pub(crate) entries: Vec<Entry<H, M>>,
    groups: Vec<Group<M>>,
    fallback: Option<H>,
    before_rules: Vec<Rule>,
    after_rules: Vec<Rule>,
}

/// Sets a route's name, constraints and middleware after it is registered.
pub struct RouteBuilder<'r, H, M> {
    router: &'r mut Router<H, M>,
    index: usize,
}

impl<H, M> RouteBuilder<'_, H, M> {
    /// Names the route; a group's name prefix is put in front.
    pub fn name(self, name: &str) -> Self {
        self.router.entries[self.index].name = Some(name.to_string());
        self
    }

    /// Constrains a parameter to `regex` (Laravel's `where`).
    pub fn where_(self, param: &str, regex: &str) -> Self {
        self.router.entries[self.index]
            .wheres
            .push((param.to_string(), regex.to_string()));
        self
    }

    /// Adds middleware after the groups' middleware.
    pub fn middleware(self, middleware: M) -> Self {
        self.router.entries[self.index].middleware.push(middleware);
        self
    }
}

impl<H, M: Clone> Default for Router<H, M> {
    fn default() -> Router<H, M> {
        Router::new()
    }
}

impl<H, M: Clone> Router<H, M> {
    pub fn new() -> Router<H, M> {
        Router {
            entries: Vec::new(),
            groups: Vec::new(),
            fallback: None,
            before_rules: Vec::new(),
            after_rules: Vec::new(),
        }
    }

    fn add(
        &mut self,
        methods: Vec<Method>,
        path: &str,
        target: Target<H>,
    ) -> RouteBuilder<'_, H, M> {
        let prefix: String = self.groups.iter().map(|g| g.prefix.as_str()).collect();
        let name_prefix: String = self.groups.iter().map(|g| g.name.as_str()).collect();
        let middleware: Vec<M> = self
            .groups
            .iter()
            .flat_map(|g| g.middleware.iter().cloned())
            .collect();
        let path = match (prefix.is_empty(), path) {
            (true, path) => path.to_string(),
            (false, "/") | (false, "") => prefix,
            (false, path) => format!("{prefix}/{}", path.trim_start_matches('/')),
        };
        self.entries.push(Entry {
            methods,
            path,
            wheres: Vec::new(),
            name_prefix,
            name: None,
            target,
            middleware,
        });
        let index = self.entries.len() - 1;
        RouteBuilder {
            router: self,
            index,
        }
    }

    pub fn get(&mut self, path: &str, handler: H) -> RouteBuilder<'_, H, M> {
        self.add(vec![Method::Get], path, Target::Handler(handler))
    }

    pub fn post(&mut self, path: &str, handler: H) -> RouteBuilder<'_, H, M> {
        self.add(vec![Method::Post], path, Target::Handler(handler))
    }

    pub fn put(&mut self, path: &str, handler: H) -> RouteBuilder<'_, H, M> {
        self.add(vec![Method::Put], path, Target::Handler(handler))
    }

    pub fn patch(&mut self, path: &str, handler: H) -> RouteBuilder<'_, H, M> {
        self.add(vec![Method::Patch], path, Target::Handler(handler))
    }

    pub fn delete(&mut self, path: &str, handler: H) -> RouteBuilder<'_, H, M> {
        self.add(vec![Method::Delete], path, Target::Handler(handler))
    }

    pub fn options(&mut self, path: &str, handler: H) -> RouteBuilder<'_, H, M> {
        self.add(vec![Method::Options], path, Target::Handler(handler))
    }

    pub fn any(&mut self, path: &str, handler: H) -> RouteBuilder<'_, H, M> {
        self.add(ALL_METHODS.to_vec(), path, Target::Handler(handler))
    }

    pub fn match_(&mut self, methods: &[Method], path: &str, handler: H) -> RouteBuilder<'_, H, M> {
        self.add(methods.to_vec(), path, Target::Handler(handler))
    }

    /// A route the router answers with a redirect, for any method, as Laravel's `Route::redirect`.
    pub fn redirect(&mut self, from: &str, to: &str, status: u16) -> RouteBuilder<'_, H, M> {
        self.add(
            ALL_METHODS.to_vec(),
            from,
            Target::Redirect {
                to: to.to_string(),
                status,
            },
        )
    }

    /// Registers the routes `routes` adds with `group`'s prefix, middleware and name prefix.
    pub fn group(&mut self, group: Group<M>, routes: impl FnOnce(&mut Router<H, M>)) {
        self.groups.push(group);
        routes(self);
        self.groups.pop();
    }

    /// The handler for requests nothing else matches.
    pub fn fallback(&mut self, handler: H) {
        self.fallback = Some(handler);
    }

    /// Sets the WordPress rule set in `slot`, replacing any earlier one.
    pub fn rules(&mut self, slot: RuleSlot, rules: Vec<Rule>) {
        match slot {
            RuleSlot::BeforeRoutes => self.before_rules = rules,
            RuleSlot::AfterRoutes => self.after_rules = rules,
        }
    }

    /// Compiles the registrations into plain route data.
    pub fn data(&self) -> Result<RouteData, CompileError> {
        let mut data = RouteData {
            before_rules: self.before_rules.clone(),
            after_rules: self.after_rules.clone(),
            ..RouteData::default()
        };
        for (i, entry) in self.entries.iter().enumerate() {
            let id = RouteId(i as u32);
            let pattern = parse(&entry.path, &entry.wheres).map_err(CompileError::Pattern)?;
            if pattern.is_static {
                for method in &entry.methods {
                    data.statics.push(StaticRoute {
                        method: method.clone(),
                        path: entry.path.clone(),
                        id,
                    });
                }
            } else {
                data.dynamics.push(DynamicRoute {
                    methods: entry.methods.clone(),
                    regex: pattern.regex,
                    params: pattern.params,
                    id,
                });
            }
            if let Some(name) = entry.full_name() {
                if data
                    .names
                    .iter()
                    .any(|(existing, _)| existing.as_str() == name)
                {
                    return Err(CompileError::DuplicateName(name));
                }
                data.names.push((RouteName::new(&name), id));
            }
        }
        Ok(data)
    }

    /// The fingerprint of [`Router::data`].
    pub fn fingerprint(&self) -> Result<Fingerprint, CompileError> {
        Ok(fingerprint_of(&self.data()?))
    }

    pub fn target(&self, id: RouteId) -> &Target<H> {
        &self.entries[id.0 as usize].target
    }

    pub fn middleware_for(&self, id: RouteId) -> &[M] {
        &self.entries[id.0 as usize].middleware
    }

    pub fn fallback_target(&self) -> Option<&H> {
        self.fallback.as_ref()
    }

    /// Every redirect route, for resolution.
    pub fn redirects(&self) -> Vec<(RouteId, String, u16)> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| match &entry.target {
                Target::Redirect { to, status } => Some((RouteId(i as u32), to.clone(), *status)),
                Target::Handler(_) => None,
            })
            .collect()
    }
}

/// SHA-256 over route data's byte form.
pub fn fingerprint_of(data: &RouteData) -> Fingerprint {
    Fingerprint(Sha256::digest(data.to_bytes()).into())
}
