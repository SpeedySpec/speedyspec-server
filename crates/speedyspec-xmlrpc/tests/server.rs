use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_http::{
    Handler, Message, Method, Middleware, Response, ServerRequestFactory, StreamFactory,
};
use speedyspec_http_message::{HttpResponse, HttpServerRequest, MessageFactory};
use speedyspec_operation::{Operation, Registries, S, Type};
use speedyspec_operation_contracts::{
    Action, Caller, Category, Identity, OperationError, RawInput, RawOutput, Value,
};
use speedyspec_operation_dispatch::{OperationHandler, bind_handler};
use speedyspec_transport::{MemoryBindingStore, TransportRegistry};
use speedyspec_transport_contracts::BindingStore;
use speedyspec_xmlrpc::codec::{parse_response, write_call};
use speedyspec_xmlrpc::{
    DefaultFaultMapping, XMLRPC, XmlRpcMiddleware, XmlRpcServer, XmlRpcTransport,
};
use speedyspec_xmlrpc_contracts::{
    Fault, MethodCall, MethodEntry, MethodResponse, XmlRpcBinding, XmlRpcHooks, XmlRpcLogin,
    XmlRpcParam,
};

type Log = Rc<RefCell<Vec<String>>>;

struct Hooks {
    log: Log,
    enabled: Cell<bool>,
    remove: RefCell<Vec<String>>,
    add: RefCell<Vec<MethodEntry>>,
    login_message: RefCell<Option<String>>,
}

impl XmlRpcHooks for Hooks {
    fn enabled(&self) -> bool {
        self.enabled.get()
    }
    fn element_limit(&self, default: usize) -> usize {
        default
    }
    fn methods(&self, table: Vec<MethodEntry>) -> Vec<MethodEntry> {
        self.log.borrow_mut().push("methods".into());
        let remove = self.remove.borrow();
        let mut table: Vec<MethodEntry> = table
            .into_iter()
            .filter(|e| !remove.contains(&e.method))
            .collect();
        table.extend(self.add.borrow().iter().cloned());
        table
    }
    fn call(&self, method: &str, params: &[Value]) -> Result<(), OperationError> {
        self.log
            .borrow_mut()
            .push(format!("xmlrpc_call:{method}:{}", params.len()));
        Ok(())
    }
    fn login_error(&self, fault: Fault, username: &str) -> Fault {
        match self.login_message.borrow().as_ref() {
            Some(message) => Fault::new(fault.code, format!("{message} {username}")),
            None => fault,
        }
    }
}

struct Login(Log);

impl XmlRpcLogin for Login {
    fn authenticate(&self, username: &str, password: &str) -> Result<Caller, OperationError> {
        self.0.borrow_mut().push(format!("login:{username}"));
        if username == "admin" && password == "pw" {
            Ok(Caller::new(Some(7), XMLRPC, Rc::new(Grants)))
        } else if username == "bob" && password == "pw" {
            Ok(Caller::new(Some(8), XMLRPC, Rc::new(Grants)))
        } else {
            Err(OperationError::new(
                "invalid_login",
                Category::Unauthenticated,
                "bad",
            ))
        }
    }
}

struct Grants;

impl Identity for Grants {
    fn can(&self, user: Option<u64>, capability: &str, _: &[Value]) -> bool {
        user == Some(7) && capability == "edit_report"
    }
}

type Answer = fn(&Value) -> Result<Value, OperationError>;

struct Neutral(Log, Answer);

impl OperationHandler for Neutral {
    fn handle(
        &self,
        _: Box<dyn std::any::Any>,
        _: &Caller,
    ) -> Result<Box<dyn std::any::Any>, OperationError> {
        unreachable!()
    }
    fn neutral(&self) -> bool {
        true
    }
    fn handle_neutral(&self, input: &Value, _: &Caller) -> Result<Value, OperationError> {
        self.0.borrow_mut().push("handler".into());
        (self.1)(input)
    }
}

struct World {
    server: Rc<XmlRpcServer>,
    hooks: Rc<Hooks>,
    log: Log,
}

fn echo(input: &Value) -> Result<Value, OperationError> {
    Ok(input.clone())
}

fn world_with(setup: impl FnOnce(&Hooks, &MemoryBindingStore)) -> World {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let r = Registries::new();
    r.register_type(
        Type::resource("report")
            .field("id", S::integer())
            .field("title", S::string())
            .build(),
    )
    .unwrap();
    let ops = [
        Operation::command("acme/update-report")
            .resource("report", Action::Update, &["id"])
            .input::<RawInput>(S::object([
                ("id", S::integer().required()),
                ("title", S::string()),
            ]))
            .output::<RawOutput>(S::any())
            .permission(|c: &Caller, _: &RawInput| c.can("edit_report", &[])),
        Operation::query("acme/hello")
            .input::<RawInput>(S::object([("name", S::string())]))
            .output::<RawOutput>(S::any())
            .public(),
        Operation::query("acme/teapot")
            .input::<RawInput>(S::object([]))
            .output::<RawOutput>(S::any())
            .public(),
        Operation::query("legacy-xmlrpc/old-method")
            .input::<RawInput>(S::any())
            .output::<RawOutput>(S::any())
            .public()
            .transports(&[XMLRPC]),
    ];
    for builder in ops {
        r.register_operation(builder.build().unwrap()).unwrap();
    }
    r.close().unwrap();
    let r = Rc::new(r);
    let container = Rc::new(Container::new());
    let handlers: [(&str, Answer); 4] = [
        ("acme/update-report", echo),
        ("acme/hello", echo),
        ("acme/teapot", |_| {
            Err(
                OperationError::new("teapot", Category::Internal, "I'm a teapot")
                    .with_details(Value::map([("fault_code", Value::Int(418))])),
            )
        }),
        ("legacy-xmlrpc/old-method", echo),
    ];
    for (name, f) in handlers {
        let h: Rc<dyn OperationHandler> = Rc::new(Neutral(log.clone(), f));
        bind_handler(&container, name, move |_, _| Ok(h.clone()));
    }
    let hooks = Rc::new(Hooks {
        log: log.clone(),
        enabled: Cell::new(true),
        remove: RefCell::new(Vec::new()),
        add: RefCell::new(Vec::new()),
        login_message: RefCell::new(None),
    });
    let bindings = Rc::new(MemoryBindingStore::new());
    bindings.bind(
        "acme/hello",
        XMLRPC,
        XmlRpcBinding {
            method: "acme.hello".into(),
            params: vec![XmlRpcParam::Field("name".into())],
        }
        .to_value(),
    );
    bindings.bind(
        "acme/teapot",
        XMLRPC,
        XmlRpcBinding {
            method: "acme.teapot".into(),
            params: vec![],
        }
        .to_value(),
    );
    bindings.bind(
        "legacy-xmlrpc/old-method",
        XMLRPC,
        XmlRpcBinding {
            method: "old.method".into(),
            params: vec![],
        }
        .to_value(),
    );
    setup(&hooks, &bindings);
    let transports = TransportRegistry::new(bindings);
    transports
        .install(Rc::new(XmlRpcTransport::new(r.clone())))
        .unwrap();
    let exposure = Rc::new(transports.expose(&r).unwrap());
    let server = Rc::new(XmlRpcServer::new(
        container,
        r,
        exposure,
        hooks.clone(),
        Rc::new(Login(log.clone())),
        Rc::new(DefaultFaultMapping),
        Rc::new(Grants),
    ));
    World { server, hooks, log }
}

fn world() -> World {
    world_with(|_, _| {})
}

fn request(method: Method, path: &str, body: &[u8]) -> HttpServerRequest {
    let request = MessageFactory
        .create_server_request(method, &format!("http://x.test{path}"), vec![])
        .unwrap();
    request.with_body(MessageFactory.create_stream(body))
}

fn send(w: &World, method: &str, params: Vec<Value>) -> MethodResponse {
    let body = write_call(&MethodCall {
        method: method.into(),
        params,
    });
    let response = w.server.serve(&request(Method::Post, "/xmlrpc.php", &body));
    assert_eq!(response.status().as_u16(), 200);
    let bytes = response.body().borrow_mut().read_all().unwrap();
    parse_response(&bytes).expect("a methodResponse")
}

fn fault(response: MethodResponse) -> Fault {
    match response {
        MethodResponse::Fault(f) => f,
        MethodResponse::Success(v) => panic!("expected a fault, got {v:?}"),
    }
}

fn success(response: MethodResponse) -> Value {
    match response {
        MethodResponse::Success(v) => v,
        MethodResponse::Fault(f) => panic!("expected a value, got {f:?}"),
    }
}

fn s(text: &str) -> Value {
    Value::String(text.into())
}

fn edit(id: i64, password: &str) -> Value {
    Value::map([
        ("methodName", s("acme.editReport")),
        (
            "params",
            Value::List(vec![
                Value::Int(1),
                s("admin"),
                s(password),
                Value::Int(id),
                Value::map([("title", s("T"))]),
            ]),
        ),
    ])
}

#[test]
fn get_is_refused_with_allow_post() {
    let w = world();
    let response = w.server.serve(&request(Method::Get, "/xmlrpc.php", b""));
    assert_eq!(response.status().as_u16(), 405);
    assert_eq!(response.headers().get("Allow").unwrap_or_default(), "POST");
    assert_eq!(
        response.headers().get("Content-Type").unwrap_or_default(),
        "text/plain"
    );
    let body = response.body().borrow_mut().read_all().unwrap();
    assert_eq!(body, b"XML-RPC server accepts POST requests only.");
}

#[test]
fn responses_are_utf8_xml() {
    let w = world();
    let body = write_call(&MethodCall {
        method: "acme.hello".into(),
        params: vec![s("x")],
    });
    let response = w.server.serve(&request(Method::Post, "/xmlrpc.php", &body));
    assert_eq!(
        response.headers().get("Content-Type").unwrap_or_default(),
        "text/xml; charset=UTF-8"
    );
}

#[test]
fn a_parse_error_is_a_fault_with_200() {
    let w = world();
    let response = w
        .server
        .serve(&request(Method::Post, "/xmlrpc.php", b"<nope"));
    assert_eq!(response.status().as_u16(), 200);
    let bytes = response.body().borrow_mut().read_all().unwrap();
    assert_eq!(
        parse_response(&bytes).unwrap(),
        MethodResponse::Fault(Fault::new(-32700, "parse error. not well formed"))
    );
}

#[test]
fn a_default_mapped_operation_answers() {
    let w = world();
    let value = success(send(
        &w,
        "acme.editReport",
        vec![
            Value::Int(1),
            s("admin"),
            s("pw"),
            Value::Int(7),
            Value::map([("title", s("T"))]),
        ],
    ));
    assert_eq!(
        value,
        Value::map([("id", Value::Int(7)), ("title", s("T"))])
    );
}

#[test]
fn positional_fields_win_over_the_struct() {
    let w = world();
    let value = success(send(
        &w,
        "acme.editReport",
        vec![
            Value::Int(1),
            s("admin"),
            s("pw"),
            Value::Int(7),
            Value::map([("id", Value::Int(9))]),
        ],
    ));
    assert_eq!(value.get("id"), Some(&Value::Int(7)));
}

#[test]
fn explicit_binding_wins_over_default() {
    let w = world_with(|_, bindings| {
        bindings.bind(
            "acme/update-report",
            XMLRPC,
            XmlRpcBinding {
                method: "acme.change".into(),
                params: vec![
                    XmlRpcParam::Username,
                    XmlRpcParam::Password,
                    XmlRpcParam::Field("id".into()),
                ],
            }
            .to_value(),
        );
    });
    assert_eq!(
        success(send(
            &w,
            "acme.change",
            vec![s("admin"), s("pw"), Value::Int(3)]
        )),
        Value::map([("id", Value::Int(3))])
    );
    assert_eq!(fault(send(&w, "acme.editReport", vec![])).code, -32601);
}

#[test]
fn missing_params_is_400_with_wordpress_message() {
    let w = world();
    assert_eq!(
        fault(send(
            &w,
            "acme.editReport",
            vec![Value::Int(1), s("admin"), s("pw")]
        )),
        Fault::new(400, "Insufficient arguments passed to this XML-RPC method.")
    );
}

#[test]
fn the_rest_struct_is_optional_and_extra_params_are_ignored() {
    let w = world();
    let value = success(send(
        &w,
        "acme.editReport",
        vec![Value::Int(1), s("admin"), s("pw"), Value::Int(7)],
    ));
    assert_eq!(value, Value::map([("id", Value::Int(7))]));
    let value = success(send(
        &w,
        "acme.hello",
        vec![s("a"), s("extra"), Value::Int(3)],
    ));
    assert_eq!(value, Value::map([("name", s("a"))]));
}

#[test]
fn a_rest_param_that_is_not_a_struct_is_400() {
    let w = world();
    let f = fault(send(
        &w,
        "acme.editReport",
        vec![
            Value::Int(1),
            s("admin"),
            s("pw"),
            Value::Int(7),
            s("not a struct"),
        ],
    ));
    assert_eq!(f.code, 400);
}

#[test]
fn operation_params_are_never_unwrapped() {
    let w = world();
    assert_eq!(
        success(send(&w, "acme.hello", vec![s("solo")])),
        Value::map([("name", s("solo"))])
    );
}

#[test]
fn validation_failure_is_400() {
    let w = world();
    let f = fault(send(
        &w,
        "acme.editReport",
        vec![
            Value::Int(1),
            s("admin"),
            s("pw"),
            Value::map([("x", Value::Int(1))]),
        ],
    ));
    assert_eq!(f.code, 400, "{f:?}");
}

#[test]
fn permission_denied_is_401() {
    let w = world();
    let f = fault(send(
        &w,
        "acme.editReport",
        vec![Value::Int(1), s("bob"), s("pw"), Value::Int(7)],
    ));
    assert_eq!(f.code, 401, "{f:?}");
}

#[test]
fn anonymous_and_refused_is_403() {
    // An explicit binding with no credentials calls anonymously; WordPress answers 403 to callers who did not log in.
    let w = world_with(|_, bindings| {
        bindings.bind(
            "acme/update-report",
            XMLRPC,
            XmlRpcBinding {
                method: "acme.anonymousEdit".into(),
                params: vec![XmlRpcParam::Field("id".into())],
            }
            .to_value(),
        );
    });
    assert_eq!(
        fault(send(&w, "acme.anonymousEdit", vec![Value::Int(1)])).code,
        403
    );
}

#[test]
fn handler_fault_code_is_kept() {
    let w = world();
    assert_eq!(
        fault(send(&w, "acme.teapot", vec![])),
        Fault::new(418, "I'm a teapot")
    );
}

#[test]
fn unknown_method_is_32601() {
    let w = world();
    assert_eq!(
        fault(send(&w, "acme.nothing", vec![])),
        Fault::new(
            -32601,
            "server error. requested method acme.nothing does not exist."
        )
    );
}

#[test]
fn removed_method_is_not_served() {
    let w = world_with(|hooks, _| hooks.remove.borrow_mut().push("acme.editReport".into()));
    assert_eq!(fault(send(&w, "acme.editReport", vec![])).code, -32601);
    let multi = success(send(
        &w,
        "system.multicall",
        vec![Value::List(vec![edit(1, "pw")])],
    ));
    assert_eq!(
        multi,
        Value::List(vec![Value::map([
            ("faultCode", Value::Int(-32601)),
            (
                "faultString",
                s("server error. requested method acme.editReport does not exist.")
            ),
        ])])
    );
    let Value::List(names) = success(send(&w, "system.listMethods", vec![])) else {
        panic!()
    };
    assert!(!names.contains(&s("acme.editReport")));
}

#[test]
fn system_methods_cannot_be_removed_or_replaced() {
    let w = world_with(|hooks, _| {
        hooks.remove.borrow_mut().push("system.multicall".into());
        hooks.add.borrow_mut().push(MethodEntry {
            method: "system.listMethods".into(),
            operation: "acme/hello".into(),
            legacy: false,
        });
    });
    let Value::List(names) = success(send(&w, "system.listMethods", vec![])) else {
        panic!()
    };
    assert!(names.contains(&s("system.multicall")));
    assert!(names.contains(&s("acme.hello")));
}

#[test]
fn list_methods_matches_ixr_order() {
    let w = world_with(|hooks, _| {
        hooks.add.borrow_mut().push(MethodEntry {
            method: "old.method".into(),
            operation: "legacy-xmlrpc/old-method".into(),
            legacy: true,
        });
    });
    let listed = success(send(&w, "system.listMethods", vec![]));
    assert_eq!(
        listed,
        Value::List(vec![
            s("system.multicall"),
            s("system.listMethods"),
            s("system.getCapabilities"),
            s("old.method"),
            s("acme.editReport"),
            s("acme.teapot"),
            s("acme.hello"),
        ])
    );
}

#[test]
fn get_capabilities_matches_ixr() {
    let w = world();
    let caps = success(send(&w, "system.getCapabilities", vec![]));
    assert_eq!(
        caps.get("xmlrpc"),
        Some(&Value::map([
            ("specUrl", s("https://xmlrpc.com/spec.md")),
            ("specVersion", Value::Int(1)),
        ]))
    );
    assert_eq!(
        caps.get("faults_interop")
            .and_then(|c| c.get("specVersion")),
        Some(&Value::Int(20010516))
    );
    assert!(caps.get("system.multicall").is_some());
}

#[test]
fn multicall_wraps_results_and_faults() {
    let w = world();
    let hello = Value::map([
        ("methodName", s("acme.hello")),
        ("params", Value::List(vec![s("a")])),
    ]);
    let nothing = Value::map([
        ("methodName", s("acme.nothing")),
        ("params", Value::List(vec![])),
    ]);
    let result = success(send(
        &w,
        "system.multicall",
        vec![Value::List(vec![hello, nothing])],
    ));
    assert_eq!(
        result,
        Value::List(vec![
            Value::List(vec![Value::map([("name", s("a"))])]),
            Value::map([
                ("faultCode", Value::Int(-32601)),
                (
                    "faultString",
                    s("server error. requested method acme.nothing does not exist.")
                ),
            ]),
        ])
    );
}

#[test]
fn nested_multicall_is_refused() {
    let w = world();
    let nested = Value::map([
        ("methodName", s("system.multicall")),
        ("params", Value::List(vec![])),
    ]);
    let result = success(send(
        &w,
        "system.multicall",
        vec![Value::List(vec![nested])],
    ));
    assert_eq!(
        result,
        Value::List(vec![Value::map([
            ("faultCode", Value::Int(-32600)),
            (
                "faultString",
                s("Recursive calls to system.multicall are forbidden")
            ),
        ])])
    );
}

#[test]
fn legacy_entries_get_their_params_as_they_came() {
    let w = world_with(|hooks, _| {
        hooks.add.borrow_mut().push(MethodEntry {
            method: "old.method".into(),
            operation: "legacy-xmlrpc/old-method".into(),
            legacy: true,
        });
    });
    assert_eq!(
        success(send(&w, "old.method", vec![Value::Int(1), s("x")])),
        Value::map([("params", Value::List(vec![Value::Int(1), s("x")]))])
    );
    assert!(
        !w.log
            .borrow()
            .iter()
            .any(|l| l.starts_with("login:") || l.starts_with("xmlrpc_call"))
    );
}

#[test]
fn disabled_server_is_405_only_for_methods_with_credentials() {
    let w = world_with(|hooks, _| hooks.enabled.set(false));
    assert_eq!(
        fault(send(
            &w,
            "acme.editReport",
            vec![Value::Int(1), s("admin"), s("pw"), Value::Int(7)]
        )),
        Fault::new(405, "XML-RPC services are disabled on this site.")
    );
    assert!(matches!(
        send(&w, "system.listMethods", vec![]),
        MethodResponse::Success(_)
    ));
    assert!(matches!(
        send(&w, "acme.hello", vec![s("a")]),
        MethodResponse::Success(_)
    ));
}

#[test]
fn one_failed_login_fails_every_later_login() {
    let w = world();
    let result = success(send(
        &w,
        "system.multicall",
        vec![Value::List(vec![
            edit(1, "wrong"),
            edit(2, "pw"),
            edit(3, "pw"),
        ])],
    ));
    let incorrect = Value::map([
        ("faultCode", Value::Int(403)),
        ("faultString", s("Incorrect username or password.")),
    ]);
    assert_eq!(
        result,
        Value::List(vec![incorrect.clone(), incorrect.clone(), incorrect])
    );
    let logins = w
        .log
        .borrow()
        .iter()
        .filter(|l| l.starts_with("login:"))
        .count();
    assert_eq!(logins, 1);
}

#[test]
fn login_error_hook_can_change_the_fault() {
    let w = world();
    *w.hooks.login_message.borrow_mut() = Some("Nope,".into());
    assert_eq!(
        fault(send(
            &w,
            "acme.editReport",
            vec![Value::Int(1), s("eve"), s("x"), Value::Int(7)]
        )),
        Fault::new(403, "Nope, eve")
    );
}

#[test]
fn xmlrpc_call_fires_after_login_before_dispatch() {
    let w = world();
    success(send(
        &w,
        "acme.editReport",
        vec![Value::Int(1), s("admin"), s("pw"), Value::Int(7)],
    ));
    let log: Vec<String> = w
        .log
        .borrow()
        .iter()
        .filter(|l| *l != "methods")
        .cloned()
        .collect();
    assert_eq!(
        log,
        ["login:admin", "xmlrpc_call:acme.editReport:4", "handler"]
    );
}

#[test]
fn the_table_is_built_once() {
    let w = world();
    send(&w, "acme.hello", vec![s("a")]);
    send(&w, "system.listMethods", vec![]);
    assert_eq!(w.log.borrow().iter().filter(|l| *l == "methods").count(), 1);
}

struct NotFound;

impl Handler<HttpServerRequest, HttpResponse> for NotFound {
    fn handle(&self, _: HttpServerRequest) -> HttpResponse {
        HttpResponse::new(speedyspec_http::StatusCode::new(404).unwrap())
    }
}

#[test]
fn the_middleware_passes_other_paths_on() {
    let w = world();
    let middleware = XmlRpcMiddleware::new(w.server.clone());
    let other = middleware.process(request(Method::Post, "/wp-json/", b""), &NotFound);
    assert_eq!(other.status().as_u16(), 404);
    let ours = middleware.process(request(Method::Get, "/xmlrpc.php", b""), &NotFound);
    assert_eq!(ours.status().as_u16(), 405);
}

#[test]
fn a_binding_without_params_uses_the_default_params() {
    let w = world_with(|_, bindings| {
        bindings.bind(
            "acme/update-report",
            XMLRPC,
            Value::map([("method", s("acme.renamed"))]),
        );
    });
    let value = success(send(
        &w,
        "acme.renamed",
        vec![
            Value::Int(1),
            s("admin"),
            s("pw"),
            Value::Int(4),
            Value::map([("title", s("T"))]),
        ],
    ));
    assert_eq!(
        value,
        Value::map([("id", Value::Int(4)), ("title", s("T"))])
    );
}
