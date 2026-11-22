//! Legacy REST routes (`register_rest_route`) as operations: the LegacyHandlerAdapter.
//!
//! Each endpoint of a route becomes an operation restricted to `rest`, with an explicit binding for the exact route
//! and an open input, so WordPress's own validation (not the Rust validator) decides what reaches the callback.
//!
//! Legacy routes belong to the request, like WordPress's `WP_REST_Server` instance: they are kept in a request-scoped
//! table that a new server instance resets, never in the site's closed registries. `RestService` serves the site's
//! operations plus this table, rebuilding only when the table changes, so a site whose plugins register the same
//! routes on every `rest_api_init` keeps one cached server.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use speedyspec_container::Lifetime;
use speedyspec_operation::{Operation, S};
use speedyspec_operation_contracts::{
    Caller, Category, OperationError, OperationSpec, PermissionCheck, RawInput, RawOutput, Value,
};
use speedyspec_operation_dispatch::{OperationHandler, bind_handler};
use speedyspec_php_ext::{Key, Value as PhpValue};
use speedyspec_rest::REST;
use speedyspec_rest_contracts::{RestBinding, RestMethod};

use crate::RestService;
use crate::errors::{Described, describe};
use crate::hooks::request_proxy;

thread_local! {
    /// This request's handler arrays (`callback`, `permission_callback`, `args`, …), by operation.
    static HANDLERS: RefCell<HashMap<String, PhpValue>> = RefCell::new(HashMap::new());
}

/// Forgets this request's handlers (the request ended).
pub fn clear() {
    HANDLERS.with(|h| h.borrow_mut().clear());
}

/// This request's handler array for `operation`.
pub fn handler(operation: &str) -> Option<PhpValue> {
    HANDLERS.with(|h| h.borrow().get(operation).cloned())
}

pub(crate) fn field(handler: &PhpValue, name: &str) -> PhpValue {
    match handler {
        PhpValue::Array(items) => items
            .iter()
            .find(|(k, _)| *k == Key::Str(name.into()))
            .map_or(PhpValue::Null, |(_, v)| v.clone()),
        _ => PhpValue::Null,
    }
}

fn fnv(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// The operation name of one legacy endpoint: `legacy-rest/{slug}-{hash}`.
pub fn operation_name(namespace: &str, route: &str, methods: &[RestMethod]) -> String {
    let methods: Vec<&str> = methods.iter().map(RestMethod::as_str).collect();
    let raw = format!("{namespace}{route}-{}", methods.join("-")).to_ascii_lowercase();
    let mut slug = String::new();
    for c in raw.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let hash = fnv(&format!(
        "{namespace}\u{0}{route}\u{0}{}",
        methods.join(",")
    ));
    format!("legacy-rest/{slug}-{hash:08x}")
}

/// The methods of a handler: a list (`['GET']`) or WordPress's normalized map (`['GET' => true]`).
fn methods_of(handler: &PhpValue) -> Vec<RestMethod> {
    match field(handler, "methods") {
        PhpValue::Array(items) => items
            .iter()
            .filter_map(|(k, m)| match (k, m) {
                (_, PhpValue::Str(name)) => RestMethod::parse(name),
                (Key::Str(name), _) => RestMethod::parse(name),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether every method reads only (`GET`, `HEAD`, `OPTIONS`), making the endpoint a query.
fn reads_only(methods: &[RestMethod]) -> bool {
    methods
        .iter()
        .all(|m| matches!(m, RestMethod::Get | RestMethod::Head | RestMethod::Options))
}

/// One legacy endpoint registered in this request.
#[derive(Clone)]
pub struct Entry {
    pub spec: OperationSpec,
    pub binding: RestBinding,
}

thread_local! {
    /// This request's legacy endpoints, by operation, in registration order. A new `WP_REST_Server` resets it.
    static ENTRIES: RefCell<IndexMap<String, Entry>> = RefCell::new(IndexMap::new());
}

/// Forgets this request's legacy routes: a new `WP_REST_Server` starts with none, as WordPress's does.
pub fn reset() {
    ENTRIES.with(|e| e.borrow_mut().clear());
    FILTERED.with(|f| f.set(false));
    clear();
}

thread_local! {
    /// Whether `rest_endpoints` has been applied to this request's legacy routes since they last changed.
    static FILTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Applies `rest_endpoints` to this request's legacy routes, as WordPress's `get_routes()` does before dispatch: a
/// route the filter removed is dropped, and a handler it changed (a tighter `permission_callback`, other `args` or
/// `callback`) is the one dispatched. Runs once per change of the routes.
pub fn apply_endpoints_filter() {
    if FILTERED.with(std::cell::Cell::get) {
        return;
    }
    FILTERED.with(|f| f.set(true));
    let Some(PhpValue::Array(routes)) = crate::errors::adapter("legacy_routes", &[]) else {
        return;
    };
    let entries: Vec<(String, Entry)> = ENTRIES.with(|e| {
        e.borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    });
    for (name, entry) in entries {
        let full = entry.binding.full_route();
        let filtered = routes
            .iter()
            .find(|(k, _)| *k == Key::Str(full.clone()))
            .map(|(_, v)| v.clone());
        let handler = match filtered {
            Some(PhpValue::Array(handlers)) => handlers.into_iter().map(|(_, h)| h).find(|h| {
                let methods = methods_of(h);
                entry.binding.methods.iter().all(|m| methods.contains(m))
            }),
            _ => None,
        };
        match handler {
            Some(handler) => {
                HANDLERS.with(|h| h.borrow_mut().insert(name, handler));
            }
            None => {
                ENTRIES.with(|e| e.borrow_mut().shift_remove(&name));
                HANDLERS.with(|h| h.borrow_mut().remove(&name));
            }
        }
    }
}

/// This request's legacy endpoints.
pub fn entries() -> Vec<Entry> {
    ENTRIES.with(|e| e.borrow().values().cloned().collect())
}

/// What identifies this request's legacy routes, so the REST server is rebuilt only when they change.
pub fn digest() -> String {
    ENTRIES.with(|e| {
        e.borrow()
            .iter()
            .map(|(name, entry)| format!("{name}={:?}", entry.binding))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// Registers one route's endpoints into this request's legacy table. `route` is the full route
/// (`/{namespace}/…`), as `WP_REST_Server::register_route` receives it. Returns the problems, for the log.
pub fn register(
    service: &RestService,
    namespace: &str,
    route: &str,
    handlers: &PhpValue,
    replace: bool,
) -> Vec<String> {
    let mut problems = Vec::new();
    let PhpValue::Array(items) = handlers else {
        return problems;
    };
    let prefix = format!("/{namespace}");
    let relative = route.strip_prefix(&prefix).unwrap_or(route).to_owned();
    if replace {
        // As WordPress's register_route with $override: the route's handlers are replaced wholesale.
        let gone: Vec<String> = ENTRIES.with(|e| {
            let mut entries = e.borrow_mut();
            let gone: Vec<String> = entries
                .iter()
                .filter(|(_, entry)| {
                    entry.binding.namespace == namespace && entry.binding.route == relative
                })
                .map(|(name, _)| name.clone())
                .collect();
            for name in &gone {
                entries.shift_remove(name);
            }
            gone
        });
        HANDLERS.with(|h| {
            let mut handlers = h.borrow_mut();
            for name in &gone {
                handlers.remove(name);
            }
        });
    }
    for (_, handler) in items {
        // Without $override, a route and method already claimed keep their first handler, as WordPress's first
        // matching handler wins: only the methods still free are registered.
        let claimed: Vec<RestMethod> = ENTRIES.with(|e| {
            e.borrow()
                .values()
                .filter(|entry| {
                    entry.binding.namespace == namespace && entry.binding.route == relative
                })
                .flat_map(|entry| entry.binding.methods.clone())
                .collect()
        });
        let methods: Vec<RestMethod> = methods_of(handler)
            .into_iter()
            .filter(|m| !claimed.contains(m))
            .collect();
        if methods.is_empty() {
            if claimed.is_empty() {
                problems.push(format!("{route}: an endpoint has no method REST serves"));
            }
            continue;
        }
        let name = operation_name(namespace, route, &methods);
        let builder = if reads_only(&methods) {
            Operation::query(&name)
        } else {
            Operation::command(&name)
        };
        let spec = match builder
            .input::<RawInput>(S::any())
            .output::<RawOutput>(S::any())
            .transports(&[REST])
            .permission_check(Rc::new(LegacyPermission {
                operation: name.clone(),
            }))
            .build()
        {
            Ok(spec) => spec,
            Err(err) => {
                problems.push(format!("{route}: {err}"));
                continue;
            }
        };
        let binding = RestBinding {
            namespace: namespace.to_owned(),
            route: relative.clone(),
            methods,
        };
        ENTRIES.with(|e| e.borrow_mut().insert(name.clone(), Entry { spec, binding }));
        FILTERED.with(|f| f.set(false));
        HANDLERS.with(|h| h.borrow_mut().insert(name.clone(), handler.clone()));
        let legacy: Rc<dyn OperationHandler> = Rc::new(LegacyHandler {
            operation: name.clone(),
        });
        let container = service.container();
        let _request = container.registering(Lifetime::Request);
        bind_handler(container, &name, move |_, _| Ok(legacy.clone()));
    }
    problems
}

fn failed(what: &str, error: impl std::fmt::Display) -> OperationError {
    eprintln!("speedyspec_wordpress: a legacy REST {what} failed: {error}");
    OperationError::new(
        "rest_callback_failed",
        Category::Internal,
        "The request could not be processed.",
    )
}

fn current_proxy() -> Result<PhpValue, OperationError> {
    let request = speedyspec_rest::current_request().ok_or_else(|| {
        OperationError::new(
            "rest_no_request",
            Category::Internal,
            "no REST request is being dispatched",
        )
    })?;
    Ok(request_proxy(&request))
}

/// The permission check of a legacy endpoint.
pub struct LegacyPermission {
    pub operation: String,
}

impl PermissionCheck for LegacyPermission {
    fn check(&self, caller: &Caller, _input: &dyn Any) -> Result<(), OperationError> {
        let handler =
            handler(&self.operation).ok_or_else(|| crate::legacy::missing(&self.operation))?;
        // As WP_REST_Server::dispatch does before respond_to_request: defaults, then the arguments' checks.
        self.prepare(&handler)?;
        let callback = field(&handler, "permission_callback");
        if crate::hooks::empty(&callback) {
            return Ok(());
        }
        let proxy = current_proxy()?;
        let allowed = speedyspec_php::call_catching(&callback, &[proxy])
            .map_err(|f| failed("permission_callback", f.error))?;
        // As WordPress's respond_to_request: a WP_Error is its error, false or null refuses, anything else allows.
        match describe(&allowed) {
            Described::Error(e) => Err(e),
            _ if !matches!(allowed, PhpValue::Bool(false) | PhpValue::Null) => Ok(()),
            _ if caller.user.is_none() => Err(OperationError::new(
                "operation_unauthenticated",
                Category::Unauthenticated,
                "Sorry, you are not allowed to do that.",
            )),
            _ => Err(OperationError::new(
                "operation_forbidden",
                Category::PermissionDenied,
                "Sorry, you are not allowed to do that.",
            )),
        }
    }
}

impl LegacyPermission {
    /// Fills the defaults from the endpoint's `args`, then runs WordPress's `has_valid_params()` and `sanitize_params()`
    /// on the request.
    fn prepare(&self, handler: &PhpValue) -> Result<(), OperationError> {
        let request = speedyspec_rest::current_request().ok_or_else(|| {
            OperationError::new(
                "rest_no_request",
                Category::Internal,
                "no REST request is being dispatched",
            )
        })?;
        if let PhpValue::Array(args) = field(handler, "args") {
            let mut r = request.borrow_mut();
            for (key, arg) in &args {
                let (Key::Str(name), default) = (key, field(arg, "default")) else {
                    continue;
                };
                if !matches!(default, PhpValue::Null)
                    && let Ok(value) = speedyspec_operation_php::convert::from_php(&default)
                {
                    r.default_params.insert(name.clone(), value);
                }
            }
        }
        let proxy =
            speedyspec_php_ext::PhpObject::new(request_proxy(&request)).ok_or_else(|| {
                OperationError::new(
                    "rest_no_request",
                    Category::Internal,
                    "the REST request proxy is not available",
                )
            })?;
        for method in ["has_valid_params", "sanitize_params"] {
            let checked = proxy.call(method, vec![]).map_err(|e| failed(method, e))?;
            if let Described::Error(e) = describe(&checked) {
                return Err(e);
            }
        }
        Ok(())
    }
}

pub(crate) fn missing(operation: &str) -> OperationError {
    OperationError::new(
        "rest_invalid_handler",
        Category::Internal,
        format!("{operation} has no callback in this request; register it on rest_api_init"),
    )
}

/// The handler of a legacy endpoint: its `callback`, given the request.
pub struct LegacyHandler {
    pub operation: String,
}

impl OperationHandler for LegacyHandler {
    fn handle(
        &self,
        _input: Box<dyn Any>,
        _caller: &Caller,
    ) -> Result<Box<dyn Any>, OperationError> {
        Err(OperationError::new(
            "handler_not_neutral",
            Category::Internal,
            "a legacy REST handler is neutral",
        ))
    }

    fn neutral(&self) -> bool {
        true
    }

    fn handle_neutral(&self, _input: &Value, _caller: &Caller) -> Result<Value, OperationError> {
        let handler = handler(&self.operation).ok_or_else(|| missing(&self.operation))?;
        let callback = field(&handler, "callback");
        let callable = speedyspec_php::call(
            &PhpValue::Str("is_callable".into()),
            std::slice::from_ref(&callback),
        );
        if !matches!(callable, Ok(PhpValue::Bool(true))) {
            return Err(OperationError::new(
                "rest_invalid_handler",
                Category::Internal,
                "The handler for the route is invalid.",
            )
            .with_details(Value::map([("status", Value::Int(500))])));
        }
        let proxy = current_proxy()?;
        let returned = speedyspec_php::call_catching(&callback, &[proxy])
            .map_err(|f| failed("callback", f.error))?;
        match describe(&returned) {
            Described::Error(e) => Err(e),
            Described::Response(response) => {
                let data = response.data.clone();
                speedyspec_rest::answer(response);
                Ok(data)
            }
            Described::Data(data) => Ok(data),
            Described::Nothing => Ok(Value::Null),
        }
    }
}
