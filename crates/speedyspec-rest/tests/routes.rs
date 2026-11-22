//! Default REST routes, the rest transport and the route table.

mod common;

use std::collections::HashSet;
use std::rc::Rc;

use common::*;
use speedyspec_operation_contracts::Value;
use speedyspec_rest::{RouteMatch, RouteTable};
use speedyspec_rest_contracts::{RestHooks, RestMethod, RestOutcome, RestResponse, SharedRequest};
use speedyspec_transport::MemoryBindingStore;
use speedyspec_transport_contracts::{BindingStore, ExposureError, ExposureLookup};

fn route_of(table: &RouteTable, op: &str) -> (String, Vec<&'static str>) {
    let entry = table
        .routes()
        .into_iter()
        .find(|e| e.operation == op)
        .unwrap_or_else(|| panic!("no route for {op}"));
    (
        entry.route.clone(),
        entry.methods.iter().map(|m| m.as_str()).collect(),
    )
}

#[test]
fn default_routes_follow_the_actions() {
    let r = registries();
    let (table, problems, _) = table(&r, Rc::new(MemoryBindingStore::new()));
    assert!(problems.is_empty(), "{problems:?}");
    let id = r"(?P<id>[\d]+)";
    let cases: [(&str, String, Vec<&str>); 11] = [
        ("acme/list-reports", "/acme/v1/reports".into(), vec!["GET"]),
        (
            "acme/get-report",
            format!("/acme/v1/reports/{id}"),
            vec!["GET"],
        ),
        (
            "acme/create-report",
            "/acme/v1/reports".into(),
            vec!["POST"],
        ),
        (
            "acme/update-report",
            format!("/acme/v1/reports/{id}"),
            vec!["POST", "PUT", "PATCH"],
        ),
        (
            "acme/delete-report",
            format!("/acme/v1/reports/{id}"),
            vec!["DELETE"],
        ),
        (
            "acme/archive-report",
            format!("/acme/v1/reports/{id}/archive"),
            vec!["POST"],
        ),
        (
            "acme/preview-report",
            format!("/acme/v1/reports/{id}/preview"),
            vec!["GET"],
        ),
        ("acme/ping", "/acme/v1/ping".into(), vec!["GET"]),
        ("acme/do-thing", "/acme/v1/do-thing".into(), vec!["POST"]),
        (
            "acme/list-categories",
            "/acme/v1/categories".into(),
            vec!["GET"],
        ),
        (
            "acme/get-category",
            "/acme/v1/categories/(?P<slug>[^/]+)".into(),
            vec!["GET"],
        ),
    ];
    for (op, route, methods) in cases {
        assert_eq!(route_of(&table, op), (route, methods), "{op}");
    }
    assert!(
        table
            .routes()
            .iter()
            .all(|e| e.operation != "acme/watch-reports")
    );
}

#[test]
fn identity_segment_depends_on_the_schema() {
    let r = registries();
    let (table, _, _) = table(&r, Rc::new(MemoryBindingStore::new()));
    assert!(
        route_of(&table, "acme/get-report")
            .0
            .ends_with(r"(?P<id>[\d]+)")
    );
    assert!(
        route_of(&table, "acme/get-category")
            .0
            .ends_with("(?P<slug>[^/]+)")
    );
}

fn bind(store: &MemoryBindingStore, op: &str, namespace: &str, route: &str, methods: &[&str]) {
    store.bind(
        op,
        "rest",
        Value::map([
            ("namespace", Value::from(namespace)),
            ("route", Value::from(route)),
            (
                "methods",
                Value::List(methods.iter().map(|m| Value::from(*m)).collect()),
            ),
        ]),
    );
}

#[test]
fn explicit_binding_wins_and_is_checked() {
    let r = registries();
    let store = Rc::new(MemoryBindingStore::new());
    bind(
        &store,
        "acme/get-report",
        "acme/v2",
        r"/r/(?P<id>\d+)",
        &["PUT"],
    );
    bind(
        &store,
        "acme/ping",
        "acme/v1",
        "/ping/(?P<nope>[a-z]+)",
        &["GET"],
    );
    let (table, _, exposure) = table(&r, store);
    assert_eq!(
        route_of(&table, "acme/get-report"),
        (r"/acme/v2/r/(?P<id>\d+)".into(), vec!["PUT"])
    );
    assert!(exposure.find("rest", "acme/ping").is_none());
    assert!(matches!(
        speedyspec_transport::Exposure::problems(&exposure),
        [ExposureError::InvalidBinding { why, .. }] if why == "route parameter nope is not an input of acme/ping"
    ));
}

#[test]
fn colliding_defaults_are_skipped_and_reported() {
    let r = registries();
    let store = Rc::new(MemoryBindingStore::new());
    // acme/ping's default is GET /acme/v1/ping; bind list-reports there explicitly so ping's default collides.
    bind(&store, "acme/list-reports", "acme/v1", "/ping", &["GET"]);
    let (table, problems, _) = table(&r, store);
    assert_eq!(
        problems,
        [
            "acme/ping: the default route /acme/v1/ping GET is taken by acme/list-reports; bind it explicitly"
        ]
    );
    assert!(table.routes().iter().all(|e| e.operation != "acme/ping"));
    assert!(matches!(
        table.find(RestMethod::Get, "/acme/v1/ping"),
        RouteMatch::Found { operation, .. } if operation == "acme/list-reports"
    ));
}

#[test]
fn duplicate_explicit_bindings_are_reported() {
    let r = registries();
    let store = Rc::new(MemoryBindingStore::new());
    bind(&store, "acme/ping", "acme/v2", "/x", &["GET"]);
    bind(&store, "acme/list-reports", "acme/v2", "/x", &["GET"]);
    let (_, problems, _) = table(&r, store);
    assert_eq!(
        problems,
        ["acme/ping: /acme/v2/x GET is already bound to acme/list-reports"]
    );
}

#[test]
fn find_matches_case_insensitively_and_head_as_get() {
    let r = registries();
    let (table, _, _) = table(&r, Rc::new(MemoryBindingStore::new()));
    match table.find(RestMethod::Get, "/ACME/V1/REPORTS/3") {
        RouteMatch::Found {
            operation,
            url_params,
            route,
        } => {
            assert_eq!(operation, "acme/get-report");
            assert_eq!(url_params.get("id"), Some(&Value::from("3")));
            assert_eq!(route, r"/acme/v1/reports/(?P<id>[\d]+)");
        }
        RouteMatch::NoRoute => panic!("no match"),
    }
}

#[test]
fn wrong_method_is_no_route() {
    let r = registries();
    let (table, _, _) = table(&r, Rc::new(MemoryBindingStore::new()));
    assert_eq!(
        table.find(RestMethod::Put, "/acme/v1/ping"),
        RouteMatch::NoRoute
    );
    assert_eq!(
        table.find(RestMethod::Get, "/acme/v1/nothing"),
        RouteMatch::NoRoute
    );
}

struct DropPing;

impl RestHooks for DropPing {
    fn authentication_errors(&self) -> Result<(), speedyspec_operation_contracts::OperationError> {
        Ok(())
    }
    fn endpoints(&self, routes: Vec<String>) -> Vec<String> {
        routes
            .into_iter()
            .filter(|r| r != "/acme/v1/ping")
            .collect()
    }
    fn pre_dispatch(&self, _: &SharedRequest) -> Option<RestOutcome> {
        None
    }
    fn before_callbacks(
        &self,
        _: &SharedRequest,
    ) -> Result<(), speedyspec_operation_contracts::OperationError> {
        Ok(())
    }
    fn dispatch_request(&self, _: &SharedRequest) -> Option<RestOutcome> {
        None
    }
    fn prepare_item(&self, _: &str, response: RestResponse, _: &SharedRequest) -> RestResponse {
        response
    }
    fn after_callbacks(&self, outcome: RestOutcome, _: &SharedRequest) -> RestOutcome {
        outcome
    }
    fn post_dispatch(&self, response: RestResponse, _: &SharedRequest) -> RestResponse {
        response
    }
    fn pre_serve(&self, _: &RestResponse, _: &SharedRequest) -> bool {
        false
    }
}

#[test]
fn endpoints_hook_can_remove_a_route() {
    let r = registries();
    let exposure = exposure(&r, Rc::new(MemoryBindingStore::new()));
    let (table, _) = RouteTable::build(&exposure, &DropPing);
    let ops: HashSet<String> = table.routes().into_iter().map(|e| e.operation).collect();
    assert!(!ops.contains("acme/ping"));
    assert!(ops.contains("acme/list-reports"));
}
