//! The XML-RPC server (Template Method): the HTTP request, the method table, system methods, login and dispatch.

use std::cell::{Cell, OnceCell};
use std::rc::Rc;

use indexmap::IndexMap;
use speedyspec_container::Container;
use speedyspec_http::{Message, Method, Request, StatusCode, StreamFactory};
use speedyspec_http_message::{HttpResponse, HttpServerRequest, MessageFactory};
use speedyspec_operation::Registries;
use speedyspec_operation_contracts::{Caller, Category, Identity, OperationError, Value};
use speedyspec_operation_dispatch::Dispatcher;
use speedyspec_transport::Exposure;
use speedyspec_transport_contracts::ExposureLookup;
use speedyspec_xmlrpc_contracts::{
    Fault, FaultMapping, MethodEntry, MethodResponse, XmlRpcBinding, XmlRpcHooks, XmlRpcLogin,
    XmlRpcMapping, XmlRpcParam,
};

use crate::codec::{Dates, parse_call, parse_call_with, write_response};
use crate::{DefaultXmlRpcMapping, XMLRPC};

/// WordPress's default `xmlrpc_element_limit`.
pub const DEFAULT_ELEMENT_LIMIT: usize = 30000;

const MULTICALL: &str = "system.multicall";
const LIST_METHODS: &str = "system.listMethods";
const GET_CAPABILITIES: &str = "system.getCapabilities";
/// IXR's own methods, in the order it adds them. Like IXR, the table cannot remove or replace them.
const SYSTEM: [&str; 3] = [GET_CAPABILITIES, LIST_METHODS, MULTICALL];

fn no_method(method: &str) -> Fault {
    Fault::new(
        -32601,
        format!("server error. requested method {method} does not exist."),
    )
}

fn insufficient() -> Fault {
    Fault::new(400, "Insufficient arguments passed to this XML-RPC method.")
}

/// A credential parameter as text, as PHP would read it.
fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Int(i)) => i.to_string(),
        Some(Value::Float(f)) => f.to_string(),
        Some(Value::Bool(true)) => "1".into(),
        _ => String::new(),
    }
}

/// Serves `xmlrpc.php` for one request: a failed login stays failed for the rest of it, as WordPress's `auth_failed`
/// does ([`XmlRpcServer::with_login_latch`] shares that flag with the request's other servers). The method table comes from [`XmlRpcHooks::methods`] the first time it is
/// needed.
pub struct XmlRpcServer {
    dispatcher: Dispatcher,
    registries: Rc<Registries>,
    mapping: Rc<dyn XmlRpcMapping>,
    exposure: Rc<Exposure>,
    hooks: Rc<dyn XmlRpcHooks>,
    login: Rc<dyn XmlRpcLogin>,
    faults: Rc<dyn FaultMapping>,
    identity: Rc<dyn Identity>,
    table: OnceCell<Vec<MethodEntry>>,
    login_failed: Rc<Cell<bool>>,
}

impl XmlRpcServer {
    /// A server for what `exposure` exposes on `xmlrpc`, dispatching through its own dispatcher over `registries` and
    /// the handlers in `container`.
    pub fn new(
        container: Rc<Container>,
        registries: Rc<Registries>,
        exposure: Rc<Exposure>,
        hooks: Rc<dyn XmlRpcHooks>,
        login: Rc<dyn XmlRpcLogin>,
        faults: Rc<dyn FaultMapping>,
        identity: Rc<dyn Identity>,
    ) -> XmlRpcServer {
        XmlRpcServer {
            dispatcher: Dispatcher::new(container, registries.clone()),
            registries,
            mapping: Rc::new(DefaultXmlRpcMapping),
            exposure,
            hooks,
            login,
            faults,
            identity,
            table: OnceCell::new(),
            login_failed: Rc::new(Cell::new(false)),
        }
    }

    /// The server sharing `latch`, the request's failed-login flag, with the request's other servers: after one
    /// failed login anywhere in the request, every later login fails without trying again.
    pub fn with_login_latch(mut self, latch: Rc<Cell<bool>>) -> Self {
        self.login_failed = latch;
        self
    }

    /// The server with another default mapping, for bindings that name a method but not its parameters.
    pub fn with_mapping(mut self, mapping: Rc<dyn XmlRpcMapping>) -> Self {
        self.mapping = mapping;
        self
    }

    /// An exposed operation's binding. A binding that names its method but not its parameters takes the default
    /// mapping's parameters.
    fn binding(&self, operation: &str) -> Option<XmlRpcBinding> {
        let exposed = self.exposure.find(XMLRPC, operation)?;
        let mut binding = XmlRpcBinding::from_value(&exposed.binding).ok()?;
        if exposed.binding.get("params").is_none() {
            binding.params = self
                .mapping
                .binding(&exposed.spec, self.registries.as_ref())
                .map(|b| b.params)
                .unwrap_or_default();
        }
        Some(binding)
    }

    /// Answers one HTTP request to `xmlrpc.php`. Faults are sent with 200, as IXR sends them.
    pub fn serve(&self, request: &HttpServerRequest) -> HttpResponse {
        if *request.method() != Method::Post {
            return respond(
                405,
                &[("Allow", "POST"), ("Content-Type", "text/plain")],
                b"XML-RPC server accepts POST requests only.",
            );
        }
        let body = request.body().borrow_mut().read_all().unwrap_or_default();
        let limit = self.hooks.element_limit(DEFAULT_ELEMENT_LIMIT);
        let response = match parse_call(&body, limit) {
            Err(fault) => MethodResponse::Fault(fault),
            Ok(call) => match self.call_with(
                &call.method,
                call.params,
                parse_call_with(&body, limit, Dates::Raw)
                    .ok()
                    .map(|raw| raw.params),
            ) {
                Ok(value) => MethodResponse::Success(value),
                Err(fault) => MethodResponse::Fault(fault),
            },
        };
        respond(
            200,
            &[("Content-Type", "text/xml; charset=UTF-8")],
            &write_response(&response),
        )
    }

    /// The served method table, built once: every operation exposed on `xmlrpc` by its method name (the first of
    /// two claiming one name wins), then [`XmlRpcHooks::methods`]. A name the hooks give twice keeps its first
    /// position and its last operation, as a PHP array key does.
    pub fn table(&self) -> &[MethodEntry] {
        self.table.get_or_init(|| {
            let mut exposed: Vec<MethodEntry> = Vec::new();
            for operation in self.exposure.exposed(XMLRPC) {
                let Ok(binding) = XmlRpcBinding::from_value(&operation.binding) else {
                    continue;
                };
                if let Some(taken) = exposed.iter().find(|e| e.method == binding.method) {
                    eprintln!(
                        "speedyspec_xmlrpc: {} and {} both claim {}; {} is not served over XML-RPC",
                        taken.operation,
                        operation.spec.name.as_str(),
                        binding.method,
                        operation.spec.name.as_str()
                    );
                    continue;
                }
                exposed.push(MethodEntry {
                    method: binding.method,
                    operation: operation.spec.name.as_str().to_owned(),
                    legacy: false,
                });
            }
            let mut table: Vec<MethodEntry> = Vec::new();
            for entry in self.hooks.methods(exposed) {
                match table.iter_mut().find(|e| e.method == entry.method) {
                    Some(slot) => *slot = entry,
                    None => table.push(entry),
                }
            }
            table
        })
    }

    /// Calls one method with its positional parameters: a system method, an operation, or a legacy method.
    pub fn call(&self, method: &str, params: Vec<Value>) -> Result<Value, Fault> {
        self.call_with(method, params, None)
    }

    /// Calls one method, with `raw`: the same parameters with their dates as the client sent them
    /// ([`Dates::Raw`]), which a legacy method receives in place of `params`.
    pub fn call_with(
        &self,
        method: &str,
        params: Vec<Value>,
        raw: Option<Vec<Value>>,
    ) -> Result<Value, Fault> {
        match method {
            GET_CAPABILITIES => return Ok(capabilities()),
            LIST_METHODS => return Ok(self.list_methods()),
            MULTICALL => return self.multicall(params, raw),
            _ => {}
        }
        let Some(entry) = self.table().iter().find(|e| e.method == method).cloned() else {
            return Err(no_method(method));
        };
        if entry.legacy {
            // A legacy method logs in and fires `xmlrpc_call` itself, as WordPress's methods do. Its raw input is
            // `{params: [...]}`, the positional parameters as they came.
            let caller = self.anonymous();
            let input = Value::map([("params", Value::List(raw.unwrap_or(params)))]);
            return self
                .dispatcher
                .call(&entry.operation, input, &caller)
                .map_err(|e| self.fault_of(&e));
        }
        self.run_operation(method, &entry.operation, params)
    }

    /// Calls the operation `operation` with positional parameters, by its binding, as its method would be called.
    pub fn call_operation(&self, operation: &str, params: Vec<Value>) -> Result<Value, Fault> {
        let binding = self
            .binding(operation)
            .ok_or_else(|| no_method(operation))?;
        self.run_operation(&binding.method, operation, params)
    }

    /// The fault an error is sent as. An internal error with no fault code of its own is logged and sent with a
    /// generic message, so no internal detail reaches the client.
    fn fault_of(&self, error: &OperationError) -> Fault {
        let fault = Fault::from_error(error, self.faults.as_ref());
        if error.category == Category::Internal && error.details.get("fault_code").is_none() {
            eprintln!("speedyspec_xmlrpc: an XML-RPC call failed: {error}");
            return Fault::new(fault.code, "The request could not be processed.");
        }
        fault
    }

    fn anonymous(&self) -> Caller {
        Caller::new(None, XMLRPC, self.identity.clone())
    }

    fn run_operation(
        &self,
        method: &str,
        operation: &str,
        params: Vec<Value>,
    ) -> Result<Value, Fault> {
        let binding = self.binding(operation).ok_or_else(|| no_method(method))?;
        let required = binding
            .params
            .iter()
            .filter(|p| **p != XmlRpcParam::Rest)
            .count();
        if params.len() < required {
            return Err(insufficient());
        }
        let mut input: IndexMap<String, Value> = IndexMap::new();
        let mut rest: Option<IndexMap<String, Value>> = None;
        let (mut username, mut password) = (None, None);
        for (param, value) in binding.params.iter().zip(params.iter()) {
            match param {
                XmlRpcParam::BlogId => {}
                XmlRpcParam::Username => username = Some(text(Some(value))),
                XmlRpcParam::Password => password = Some(text(Some(value))),
                XmlRpcParam::Field(field) => {
                    input.insert(field.clone(), value.clone());
                }
                XmlRpcParam::Rest => match value {
                    Value::Map(map) => rest = Some(map.clone()),
                    _ => {
                        return Err(Fault::new(
                            400,
                            "Invalid arguments passed to this XML-RPC method: the last argument must be a struct.",
                        ));
                    }
                },
            }
        }
        // Positional fields win over the same keys in the struct.
        for (key, value) in rest.into_iter().flatten() {
            input.entry(key).or_insert(value);
        }
        let caller = if username.is_some() || password.is_some() {
            self.login(
                username.as_deref().unwrap_or_default(),
                password.as_deref().unwrap_or_default(),
            )?
        } else {
            self.anonymous()
        };
        self.hooks
            .call(method, &params)
            .map_err(|e| self.fault_of(&e))?;
        self.dispatcher
            .call(operation, Value::Map(input), &caller)
            .map_err(|e| self.fault_of(&e))
    }

    /// Logs in, as WordPress's `wp_xmlrpc_server::login` does: 405 when XML-RPC is disabled, 403 (through
    /// `xmlrpc_login_error`) on bad credentials, and 403 without trying again after one failure in this request.
    pub fn login(&self, username: &str, password: &str) -> Result<Caller, Fault> {
        if !self.hooks.enabled() {
            return Err(Fault::new(
                405,
                "XML-RPC services are disabled on this site.",
            ));
        }
        let incorrect = || Fault::new(403, "Incorrect username or password.");
        if self.login_failed.get() {
            return Err(self.hooks.login_error(incorrect(), username));
        }
        match self.login.authenticate(username, password) {
            Ok(mut caller) => {
                caller.transport = XMLRPC.to_owned();
                Ok(caller)
            }
            Err(_) => {
                self.login_failed.set(true);
                Err(self.hooks.login_error(incorrect(), username))
            }
        }
    }

    /// IXR's `system.listMethods`: the table's names and the system methods (which keep a table entry's position),
    /// reversed so the user-defined methods come last-registered first.
    fn list_methods(&self) -> Value {
        let mut names: Vec<String> = self.table().iter().map(|e| e.method.clone()).collect();
        for system in SYSTEM {
            if !names.iter().any(|n| n == system) {
                names.push(system.to_owned());
            }
        }
        Value::List(names.into_iter().rev().map(Value::String).collect())
    }

    /// IXR's `system.multicall`: `[value]` for each success, `{faultCode, faultString}` for each fault.
    fn multicall(&self, params: Vec<Value>, raw: Option<Vec<Value>>) -> Result<Value, Fault> {
        let mut raw_calls = match raw.and_then(|raw| raw.into_iter().next()) {
            Some(Value::List(calls)) => Some(calls.into_iter()),
            _ => None,
        };
        let calls = match params.into_iter().next() {
            Some(Value::List(calls)) => calls,
            _ => {
                return Err(Fault::new(
                    -32600,
                    "server error. invalid xml-rpc. system.multicall expects an array of calls",
                ));
            }
        };
        let results = calls
            .into_iter()
            .map(|call| {
                let params_of = |call: &Value| match call.get("params") {
                    None | Some(Value::Null) => Vec::new(),
                    Some(Value::List(params)) => params.clone(),
                    Some(other) => vec![other.clone()],
                };
                let method = text(call.get("methodName"));
                let params = params_of(&call);
                let raw = raw_calls
                    .as_mut()
                    .and_then(Iterator::next)
                    .map(|raw| params_of(&raw));
                let result = if method == MULTICALL {
                    Err(Fault::new(
                        -32600,
                        "Recursive calls to system.multicall are forbidden",
                    ))
                } else {
                    self.call_with(&method, params, raw)
                };
                match result {
                    Ok(value) => Value::List(vec![value]),
                    Err(fault) => Value::map([
                        ("faultCode", Value::Int(fault.code)),
                        ("faultString", Value::String(fault.message)),
                    ]),
                }
            })
            .collect();
        Ok(Value::List(results))
    }
}

/// IXR's capabilities, word for word.
fn capabilities() -> Value {
    let spec = |url: &str, version: i64| {
        Value::map([
            ("specUrl", Value::String(url.into())),
            ("specVersion", Value::Int(version)),
        ])
    };
    Value::map([
        ("xmlrpc", spec("https://xmlrpc.com/spec.md", 1)),
        (
            "faults_interop",
            spec(
                "https://web.archive.org/web/20240416231938/https://xmlrpc-epi.sourceforge.net/specs/rfc.fault_codes.php",
                20010516,
            ),
        ),
        (
            "system.multicall",
            spec(
                "https://web.archive.org/web/20060624230303/http://www.xmlrpc.com/discuss/msgReader$1208?mode=topic",
                1,
            ),
        ),
    ])
}

pub(crate) fn respond(status: u16, headers: &[(&str, &str)], body: &[u8]) -> HttpResponse {
    let mut response = HttpResponse::new(StatusCode::new(status).expect("a valid status"));
    for (name, value) in headers {
        response = response
            .with_header(name, vec![(*value).to_owned()])
            .expect("a valid header");
    }
    response.with_body(MessageFactory.create_stream(body))
}

/// An error category for an XML-RPC fault code: the inverse of WordPress's codes, `Internal` for any other.
pub fn category_of(code: i64) -> Category {
    match code {
        400 => Category::InvalidArgument,
        401 => Category::PermissionDenied,
        403 => Category::Unauthenticated,
        404 => Category::NotFound,
        405 | 503 => Category::Unavailable,
        409 => Category::Conflict,
        429 => Category::ResourceExhausted,
        _ => Category::Internal,
    }
}

/// An operation error that keeps an XML-RPC fault's code and message.
pub fn error_of(fault: &Fault) -> OperationError {
    OperationError::new(
        "xmlrpc_fault",
        category_of(fault.code),
        fault.message.clone(),
    )
    .with_details(Value::map([("fault_code", Value::Int(fault.code))]))
}
