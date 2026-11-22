//! _links and _embed from type relations.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::*;
use serde_json::json;
use speedyspec_operation::{Operation, Registries, S, Type};
use speedyspec_operation_contracts::{Action, Category, OperationError, Value};
use speedyspec_rest::{NoRestHooks, RouteTable, encode, errors, links};
use speedyspec_rest_contracts::{RestLink, RestResponse};
use speedyspec_transport::MemoryBindingStore;

const BASE: &str = "http://x.test/wp-json";

fn setup() -> (Rc<Registries>, RouteTable) {
    let r = Registries::new();
    r.register_type(
        Type::resource("post")
            .field("id", S::integer())
            .field("author_id", S::integer())
            .relation("author", "core/get-user", [("id", "author_id")])
            .relation("comments", "core/list-comments", [("post", "id")])
            .build(),
    )
    .unwrap();
    r.register_type(Type::resource("user").field("id", S::integer()).build())
        .unwrap();
    r.register_type(Type::resource("comment").field("id", S::integer()).build())
        .unwrap();
    for builder in [
        op(
            Operation::query("core/get-user").resource("user", Action::Get, &["id"]),
            id(),
        ),
        op(
            Operation::query("core/get-post").resource("post", Action::Get, &["id"]),
            id(),
        ),
        op(
            Operation::query("core/list-comments").resource("comment", Action::List, &[]),
            S::object([("post", S::integer())]),
        ),
    ] {
        r.register_operation(builder.build().unwrap()).unwrap();
    }
    r.close().unwrap();
    let r = Rc::new(r);
    let exposure = exposure(&r, Rc::new(MemoryBindingStore::new()));
    let (table, problems) = RouteTable::build(&exposure, &NoRestHooks);
    assert!(problems.is_empty(), "{problems:?}");
    (r, table)
}

fn item(author: Option<i64>) -> Value {
    let mut map = vec![("id", Value::Int(3))];
    if let Some(a) = author {
        map.push(("author_id", Value::Int(a)));
    }
    Value::map(map)
}

fn link(rel: &str, href: &str, embeddable: bool) -> RestLink {
    RestLink {
        rel: rel.into(),
        href: href.into(),
        embeddable,
    }
}

#[test]
fn relation_links_fill_routes_from_fields() {
    let (r, table) = setup();
    let found = links::for_item("post", &item(Some(9)), r.as_ref(), r.as_ref(), &table, BASE);
    assert!(
        found.contains(&link(
            "author",
            "http://x.test/wp-json/core/v1/users/9",
            true
        )),
        "{found:?}"
    );
    assert!(
        found.contains(&link(
            "self",
            "http://x.test/wp-json/core/v1/posts/3",
            false
        )),
        "{found:?}"
    );
}

#[test]
fn non_route_args_go_in_the_query() {
    let (r, table) = setup();
    let found = links::for_item("post", &item(Some(9)), r.as_ref(), r.as_ref(), &table, BASE);
    assert!(
        found.contains(&link(
            "comments",
            "http://x.test/wp-json/core/v1/comments?post=3",
            true
        )),
        "{found:?}"
    );
}

#[test]
fn missing_source_field_has_no_link() {
    let (r, table) = setup();
    let found = links::for_item("post", &item(None), r.as_ref(), r.as_ref(), &table, BASE);
    assert!(found.iter().all(|l| l.rel != "author"), "{found:?}");
}

#[test]
fn embed_puts_data_or_error_under_embedded() {
    let mut response = RestResponse::ok(item(Some(9)));
    response.links = vec![
        link("self", "http://x.test/wp-json/core/v1/posts/3", false),
        link("author", "http://x.test/wp-json/core/v1/users/9", true),
        link(
            "comments",
            "http://x.test/wp-json/core/v1/comments?post=3",
            true,
        ),
    ];
    let calls = RefCell::new(Vec::new());
    links::embed(&mut response, &|href: &str| {
        calls.borrow_mut().push(href.to_owned());
        if href.contains("users") {
            RestResponse::ok(Value::map([("id", Value::Int(9))]))
        } else {
            errors::error_response(&OperationError::new(
                "operation_forbidden",
                Category::PermissionDenied,
                "no",
            ))
        }
    });
    assert_eq!(calls.borrow().len(), 2);
    let data = encode::to_json(&response.data);
    assert_eq!(data["_embedded"]["author"], json!([{"id": 9}]));
    assert_eq!(data["_embedded"]["comments"][0]["code"], "rest_forbidden");
    assert_eq!(
        data["_links"]["self"],
        json!([{"href": "http://x.test/wp-json/core/v1/posts/3"}])
    );
    assert_eq!(
        data["_links"]["author"],
        json!([{"href": "http://x.test/wp-json/core/v1/users/9", "embeddable": true}])
    );
}

#[test]
fn render_links_without_embedding() {
    let mut response = RestResponse::ok(item(Some(9)));
    response.links = vec![link(
        "author",
        "http://x.test/wp-json/core/v1/users/9",
        true,
    )];
    links::render(&mut response);
    let data = encode::to_json(&response.data);
    assert!(data.get("_embedded").is_none());
    assert_eq!(data["_links"]["author"][0]["embeddable"], true);
}
