//! RestServer: WordPress's serve order, short-circuits, writes, statuses, embeds and the index.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use serde_json::json;
use speedyspec_container::Container;
use speedyspec_http::{Handler, Message, Method, Middleware, Response, ServerRequestFactory};
use speedyspec_http_message::{HttpResponse, HttpServerRequest, MessageFactory};
use speedyspec_operation::{Operation, Registries, S, Type};
use speedyspec_operation_contracts::{
    Action, Caller, Category, Identity, OperationError, RawInput, RawOutput, Value,
};
use speedyspec_operation_dispatch::{OperationHandler, bind_handler};
use speedyspec_rest::{RestMiddleware, RestServer, RestTransport};
use speedyspec_rest_contracts::{RestHooks, RestOutcome, RestResponse, SharedRequest};
use speedyspec_transport::{MemoryBindingStore, TransportRegistry};
use speedyspec_transport_contracts::Authenticator;

type Log = Rc<RefCell<Vec<String>>>;

/// What a scripted hook does at one point.
#[derive(Clone)]
enum Script {
    Fail(Category),
    Answer(Value),
    SetParam(&'static str, Value),
    Replace(Value),
}

#[derive(Default)]
struct RecordingHooks {
    log: Log,
    scripts: RefCell<HashMap<&'static str, Script>>,
}

impl RecordingHooks {
    fn new(log: Log) -> Self {
        RecordingHooks {
            log,
            scripts: RefCell::new(HashMap::new()),
        }
    }
    fn script(&self, hook: &'static str, script: Script) {
        self.scripts.borrow_mut().insert(hook, script);
    }
    fn hit(&self, hook: &str) -> Option<Script> {
        self.log.borrow_mut().push(hook.to_owned());
        self.scripts
            .borrow()
            .iter()
            .find(|(k, _)| **k == hook)
            .map(|(_, v)| v.clone())
    }
    fn fail(category: Category) -> OperationError {
        OperationError::new("scripted", category, "scripted")
    }
}

impl RestHooks for RecordingHooks {
    fn authentication_errors(&self) -> Result<(), OperationError> {
        match self.hit("authentication_errors") {
            Some(Script::Fail(c)) => Err(Self::fail(c)),
            _ => Ok(()),
        }
    }
    fn endpoints(&self, routes: Vec<String>) -> Vec<String> {
        routes
    }
    fn pre_dispatch(&self, _: &SharedRequest) -> Option<RestOutcome> {
        match self.hit("pre_dispatch") {
            Some(Script::Answer(v)) => Some(Ok(RestResponse::ok(v))),
            _ => None,
        }
    }
    fn before_callbacks(&self, request: &SharedRequest) -> Result<(), OperationError> {
        match self.hit("before_callbacks") {
            Some(Script::Fail(c)) => Err(Self::fail(c)),
            Some(Script::SetParam(k, v)) => {
                request.borrow_mut().set_param(k, v);
                Ok(())
            }
            _ => Ok(()),
        }
    }
    fn dispatch_request(&self, _: &SharedRequest) -> Option<RestOutcome> {
        match self.hit("dispatch_request") {
            Some(Script::Answer(v)) => Some(Ok(RestResponse {
                status: 202,
                ..RestResponse::ok(v)
            })),
            _ => None,
        }
    }
    fn prepare_item(
        &self,
        type_name: &str,
        response: RestResponse,
        _: &SharedRequest,
    ) -> RestResponse {
        self.hit(&format!("prepare_item:{type_name}"));
        response
    }
    fn after_callbacks(&self, outcome: RestOutcome, _: &SharedRequest) -> RestOutcome {
        match (self.hit("after_callbacks"), &outcome) {
            (Some(Script::Replace(v)), Err(_)) => Ok(RestResponse::ok(v)),
            _ => outcome,
        }
    }
    fn post_dispatch(&self, response: RestResponse, _: &SharedRequest) -> RestResponse {
        self.hit("post_dispatch");
        response
    }
    fn pre_serve(&self, _: &RestResponse, _: &SharedRequest) -> bool {
        self.hit("pre_serve");
        false
    }
}

/// Grants `read_report` to user 7, logging each check.
struct RecordingIdentity(Log);

impl Identity for RecordingIdentity {
    fn can(&self, user: Option<u64>, capability: &str, _: &[Value]) -> bool {
        self.0.borrow_mut().push("permission".into());
        user == Some(7) && capability == "read_report"
    }
}

struct Auth(Option<u64>, Rc<dyn Identity>);

impl Authenticator<HttpServerRequest> for Auth {
    fn authenticate(&self, _: &HttpServerRequest) -> Result<Caller, OperationError> {
        Ok(Caller::new(self.0, "rest", self.1.clone()))
    }
}

type Answer = fn(&Value) -> Value;

/// A neutral handler answering with `f(input)`, logging `handler`.
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
        Ok((self.1)(input))
    }
}

fn report(input: &Value) -> Value {
    let id = input.get("id").cloned().unwrap_or(Value::Int(1));
    let title = input.get("title").cloned().unwrap_or(Value::from("t"));
    Value::map([("id", id), ("title", title), ("author_id", Value::Int(9))])
}

fn user(input: &Value) -> Value {
    Value::map([("id", input.get("id").cloned().unwrap_or(Value::Null))])
}

struct World {
    server: Rc<RestServer>,
    hooks: Rc<RecordingHooks>,
    log: Log,
}

fn world(user_id: Option<u64>) -> World {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let r = Registries::new();
    r.register_type(
        Type::resource("report")
            .field("id", S::integer())
            .field("title", S::string())
            .field("author_id", S::integer())
            .relation("author", "core/get-user", [("id", "author_id")])
            .relation("secret", "acme/get-secret", [("id", "id")])
            .build(),
    )
    .unwrap();
    r.register_type(Type::resource("user").field("id", S::integer()).build())
        .unwrap();
    r.register_type(Type::resource("secret").field("id", S::integer()).build())
        .unwrap();
    let get_input = || S::object([("id", S::integer().required()), ("title", S::string())]);
    let ops = [
        Operation::query("acme/get-report")
            .resource("report", Action::Get, &["id"])
            .input::<RawInput>(get_input())
            .output::<RawOutput>(Type::reference("report"))
            .permission(|c: &Caller, _: &RawInput| c.can("read_report", &[])),
        Operation::command("acme/create-report")
            .resource("report", Action::Create, &[])
            .input::<RawInput>(S::object([("title", S::string())]))
            .output::<RawOutput>(Type::reference("report"))
            .public(),
        Operation::query("acme/list-reports")
            .resource("report", Action::List, &[])
            .input::<RawInput>(S::object([]))
            .output::<RawOutput>(S::array(Type::reference("report")))
            .public(),
        Operation::query("core/get-user")
            .resource("user", Action::Get, &["id"])
            .input::<RawInput>(S::object([("id", S::integer().required())]))
            .output::<RawOutput>(Type::reference("user"))
            .public(),
        Operation::query("acme/get-secret")
            .resource("secret", Action::Get, &["id"])
            .input::<RawInput>(S::object([("id", S::integer().required())]))
            .output::<RawOutput>(Type::reference("secret"))
            .permission(|_: &Caller, _: &RawInput| false),
    ];
    for builder in ops {
        r.register_operation(builder.build().unwrap()).unwrap();
    }
    r.close().unwrap();
    let r = Rc::new(r);
    let container = Rc::new(Container::new());
    let handlers: [(&str, Answer); 5] = [
        ("acme/get-report", report),
        ("acme/create-report", report),
        ("acme/list-reports", |_| {
            Value::List(vec![
                report(&Value::map([("id", Value::Int(1))])),
                report(&Value::map([("id", Value::Int(2))])),
            ])
        }),
        ("core/get-user", user),
        ("acme/get-secret", user),
    ];
    for (name, f) in handlers {
        let h: Rc<dyn OperationHandler> = Rc::new(Neutral(log.clone(), f));
        bind_handler(&container, name, move |_, _| Ok(h.clone()));
    }
    let transports = TransportRegistry::new(Rc::new(MemoryBindingStore::new()));
    transports
        .install(Rc::new(RestTransport::new(r.clone())))
        .unwrap();
    let exposure = Rc::new(transports.expose(&r).unwrap());
    let hooks = Rc::new(RecordingHooks::new(log.clone()));
    let identity: Rc<dyn Identity> = Rc::new(RecordingIdentity(log.clone()));
    let server = Rc::new(RestServer::new(
        container,
        r,
        exposure,
        hooks.clone(),
        Rc::new(Auth(user_id, identity)),
        "http://x.test/wp-json",
    ));
    World { server, hooks, log }
}

fn get(w: &World, method: Method, path: &str) -> HttpResponse {
    let request = MessageFactory
        .create_server_request(method, &format!("http://x.test{path}"), vec![])
        .unwrap();
    w.server.serve(&request).expect("a REST request")
}

fn json_of(response: &HttpResponse) -> serde_json::Value {
    let body = response.body().borrow_mut().read_all().unwrap();
    serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null)
}

fn status(response: &HttpResponse) -> u16 {
    response.status().as_u16()
}

#[test]
fn hooks_fire_in_trunk_order() {
    let w = world(Some(7));
    let r = get(&w, Method::Get, "/wp-json/acme/v1/reports/3");
    assert_eq!(status(&r), 200, "{:?}", json_of(&r));
    assert_eq!(
        *w.log.borrow(),
        [
            "authentication_errors",
            "pre_dispatch",
            "before_callbacks",
            "permission",
            "dispatch_request",
            "handler",
            "prepare_item:report",
            "after_callbacks",
            "post_dispatch",
            "pre_serve"
        ]
    );
}

#[test]
fn authentication_error_answers_its_status() {
    let w = world(Some(7));
    w.hooks.script(
        "authentication_errors",
        Script::Fail(Category::Unauthenticated),
    );
    let r = get(&w, Method::Get, "/wp-json/acme/v1/reports/3");
    assert_eq!(status(&r), 401);
    assert_eq!(w.log.borrow()[0], "authentication_errors");
    assert!(!w.log.borrow().contains(&"pre_dispatch".to_owned()));
}

#[test]
fn pre_dispatch_short_circuits_before_matching() {
    let w = world(Some(7));
    w.hooks
        .script("pre_dispatch", Script::Answer(Value::from("early")));
    let r = get(&w, Method::Get, "/wp-json/no/v1/route");
    assert_eq!((status(&r), json_of(&r)), (200, json!("early")));
    assert!(!w.log.borrow().contains(&"handler".to_owned()));
}

#[test]
fn before_callbacks_error_skips_permission_and_handler_but_not_after_callbacks() {
    let w = world(Some(7));
    w.hooks
        .script("before_callbacks", Script::Fail(Category::Conflict));
    let r = get(&w, Method::Get, "/wp-json/acme/v1/reports/3");
    assert_eq!(status(&r), 409);
    let log = w.log.borrow();
    assert!(!log.contains(&"permission".to_owned()) && !log.contains(&"handler".to_owned()));
    assert!(log.contains(&"after_callbacks".to_owned()));
}

#[test]
fn before_callbacks_write_reaches_the_handler() {
    let w = world(Some(7));
    w.hooks.script(
        "before_callbacks",
        Script::SetParam("title", Value::from("changed")),
    );
    let r = get(&w, Method::Get, "/wp-json/acme/v1/reports/3");
    assert_eq!(json_of(&r)["title"], "changed");
    let w = world(Some(7));
    w.hooks
        .script("before_callbacks", Script::SetParam("id", Value::from("x")));
    let r = get(&w, Method::Get, "/wp-json/acme/v1/reports/3");
    assert_eq!(
        (status(&r), json_of(&r)["code"].clone()),
        (400, json!("rest_invalid_param"))
    );
}

#[test]
fn dispatch_request_short_circuits_after_permission() {
    let w = world(Some(7));
    w.hooks.script(
        "dispatch_request",
        Script::Answer(Value::map([
            ("id", Value::Int(5)),
            ("title", Value::from("short")),
        ])),
    );
    let r = get(&w, Method::Get, "/wp-json/acme/v1/reports/3");
    assert_eq!(
        (status(&r), json_of(&r)["title"].clone()),
        (202, json!("short"))
    );
    let log = w.log.borrow();
    assert!(log.contains(&"permission".to_owned()) && !log.contains(&"handler".to_owned()));
}

#[test]
fn after_callbacks_sees_errors_and_may_replace_them() {
    let w = world(Some(8));
    w.hooks
        .script("after_callbacks", Script::Replace(Value::from("rescued")));
    let r = get(&w, Method::Get, "/wp-json/acme/v1/reports/3");
    assert_eq!((status(&r), json_of(&r)), (200, json!("rescued")));
}

#[test]
fn permission_refusals_are_401_or_403() {
    let anonymous = world(None);
    let r = get(&anonymous, Method::Get, "/wp-json/acme/v1/reports/3");
    assert_eq!(
        (status(&r), json_of(&r)["code"].clone()),
        (401, json!("rest_forbidden"))
    );
    let other = world(Some(8));
    assert_eq!(
        status(&get(&other, Method::Get, "/wp-json/acme/v1/reports/3")),
        403
    );
}

#[test]
fn create_answers_201() {
    let w = world(None);
    assert_eq!(
        status(&get(&w, Method::Post, "/wp-json/acme/v1/reports")),
        201
    );
}

#[test]
fn unknown_path_and_wrong_method_are_rest_no_route_404() {
    let w = world(Some(7));
    for (method, path) in [
        (Method::Get, "/wp-json/acme/v1/nothing"),
        (Method::Delete, "/wp-json/acme/v1/reports/3"),
    ] {
        let r = get(&w, method, path);
        assert_eq!(
            (status(&r), json_of(&r)["code"].clone()),
            (404, json!("rest_no_route")),
            "{path}"
        );
    }
}

#[test]
fn head_is_get_without_body() {
    let w = world(Some(7));
    let r = get(&w, Method::Head, "/wp-json/acme/v1/reports/3");
    assert_eq!(status(&r), 200);
    assert!(r.body().borrow_mut().read_all().unwrap().is_empty());
}

#[test]
fn links_and_embed_run_through_dispatch_with_the_same_caller() {
    let w = world(Some(7));
    let plain = json_of(&get(&w, Method::Get, "/wp-json/acme/v1/reports/3"));
    assert_eq!(
        plain["_links"]["author"][0]["href"],
        "http://x.test/wp-json/core/v1/users/9"
    );
    assert!(plain.get("_embedded").is_none());
    let embedded = json_of(&get(&w, Method::Get, "/wp-json/acme/v1/reports/3?_embed=1"));
    assert_eq!(embedded["_embedded"]["author"][0]["id"], 9);
    assert_eq!(embedded["_embedded"]["secret"][0]["code"], "rest_forbidden");
    assert_eq!(embedded["_embedded"]["secret"][0]["data"]["status"], 403);
}

#[test]
fn lists_prepare_and_link_each_item() {
    let w = world(None);
    let r = json_of(&get(&w, Method::Get, "/wp-json/acme/v1/reports"));
    assert_eq!(
        r[1]["_links"]["self"][0]["href"],
        "http://x.test/wp-json/acme/v1/reports/2"
    );
    assert_eq!(
        w.log
            .borrow()
            .iter()
            .filter(|l| *l == "prepare_item:report")
            .count(),
        2
    );
}

#[test]
fn fields_limit_the_response() {
    let w = world(Some(7));
    let r = json_of(&get(
        &w,
        Method::Get,
        "/wp-json/acme/v1/reports/3?_fields=id",
    ));
    assert_eq!(r, json!({"id": 3}));
}

#[test]
fn index_lists_namespaces_and_routes() {
    let w = world(None);
    let index = json_of(&get(&w, Method::Get, "/wp-json/"));
    let namespaces = index["namespaces"].as_array().unwrap();
    assert!(namespaces.contains(&json!("acme/v1")) && namespaces.contains(&json!("core/v1")));
    let route = &index["routes"]["/acme/v1/reports/(?P<id>[\\d]+)"];
    assert_eq!(route["namespace"], "acme/v1");
    assert_eq!(
        route["endpoints"][0]["args"]["id"],
        json!({"type": "integer", "required": true})
    );
    let ns = json_of(&get(&w, Method::Get, "/wp-json/core/v1"));
    assert_eq!(ns["namespace"], "core/v1");
    assert!(
        ns["routes"]
            .as_object()
            .unwrap()
            .keys()
            .all(|k| k.starts_with("/core/v1"))
    );
}

struct Next;

impl Handler<HttpServerRequest, HttpResponse> for Next {
    fn handle(&self, _: HttpServerRequest) -> HttpResponse {
        HttpResponse::new(speedyspec_http::StatusCode::new(299).unwrap())
    }
}

#[test]
fn middleware_passes_non_rest_requests_on() {
    let w = world(None);
    let m = RestMiddleware::new(w.server.clone());
    let about = MessageFactory
        .create_server_request(Method::Get, "http://x.test/about", vec![])
        .unwrap();
    assert_eq!(m.process(about, &Next).status().as_u16(), 299);
    let rest = MessageFactory
        .create_server_request(Method::Get, "http://x.test/wp-json/acme/v1/reports", vec![])
        .unwrap();
    assert_eq!(m.process(rest, &Next).status().as_u16(), 200);
}
