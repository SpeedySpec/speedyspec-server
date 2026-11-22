//! Calling operations by name.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::*;
use speedyspec_container::Container;
use speedyspec_operation::Registries;
use speedyspec_operation_contracts::*;
use speedyspec_operation_dispatch::*;

fn dispatcher(handler: Rc<dyn OperationHandler>) -> Dispatcher {
    let c = Rc::new(Container::new());
    bind_handler(&c, "core/get-post", move |_, _| Ok(handler.clone()));
    Dispatcher::new(c, closed_registries())
}

fn ok_handler() -> Rc<dyn OperationHandler> {
    handler_fn(|i: GetPost, _: &Caller| {
        Ok(Post {
            title: format!("post {}", i.id),
        })
    })
}

fn input(id: i64) -> Value {
    Value::map([("id", Value::from(id))])
}

#[test]
fn calls_a_handler_by_name_with_neutral_values() {
    let out = dispatcher(ok_handler())
        .call("core/get-post", input(7), &caller(Some(1)))
        .unwrap();
    assert_eq!(out, Value::map([("title", Value::from("post 7"))]));
}

#[test]
fn invalid_input_reports_violations() {
    let e = dispatcher(ok_handler())
        .call("core/get-post", Value::map::<&str>([]), &caller(Some(1)))
        .unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.category),
        ("invalid_input", Category::InvalidArgument)
    );
    let Value::List(v) = &*e.details else {
        panic!()
    };
    assert_eq!(v[0].get("rule"), Some(&Value::from("required")));
}

#[test]
fn permission_runs_before_the_handler() {
    let ran = Rc::new(RefCell::new(false));
    let seen = ran.clone();
    let d = dispatcher(handler_fn(move |_: GetPost, _: &Caller| {
        *seen.borrow_mut() = true;
        Ok(Post { title: "x".into() })
    }));
    assert_eq!(
        d.call("core/get-post", input(1), &caller(None))
            .unwrap_err()
            .category,
        Category::Unauthenticated
    );
    assert!(!*ran.borrow());
}

#[test]
fn output_of_the_wrong_type_is_internal() {
    let d = dispatcher(handler_fn(|_: GetPost, _: &Caller| Ok(42i64)));
    let e = d
        .call("core/get-post", input(1), &caller(Some(1)))
        .unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.category),
        ("output_type_mismatch", Category::Internal)
    );
}

#[test]
fn output_that_breaks_its_schema_is_internal_and_never_returned() {
    let c = Rc::new(Container::new());
    bind_handler(&c, "core/get-titled", |_, _| {
        Ok(handler_fn(|_: RawInput, _: &Caller| {
            Ok(Post {
                title: String::new(),
            })
        }))
    });
    let e = Dispatcher::new(c, titled_registries())
        .call(
            "core/get-titled",
            RawInput::default().extract(),
            &caller(Some(1)),
        )
        .unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.category),
        ("invalid_output", Category::Internal)
    );
    let Value::List(v) = &*e.details else {
        panic!()
    };
    assert_eq!(v[0].get("rule"), Some(&Value::from("minLength")));
}

#[test]
fn unknown_and_restricted_operations_are_not_found() {
    let d = dispatcher(ok_handler());
    assert_eq!(
        d.call("core/nope", input(1), &caller(Some(1)))
            .unwrap_err()
            .code,
        "operation_not_found"
    );
    let c = Rc::new(Container::new());
    bind_handler(&c, "core/only-rest", |_, _| {
        Ok(handler_fn(|i: RawInput, _: &Caller| {
            Ok(RawOutput {
                value: i.args,
                ..Default::default()
            })
        }))
    });
    let d2 = Dispatcher::new(c, restricted_registries());
    let identity: Rc<dyn Identity> = Rc::new(speedyspec_operation::identity::MemoryIdentity::new());
    let raw = RawInput::default().extract();
    assert_eq!(
        d2.call(
            "core/only-rest",
            raw.clone(),
            &Caller::new(Some(1), "xmlrpc", identity.clone())
        )
        .unwrap_err()
        .code,
        "operation_not_found"
    );
    assert!(
        d2.call(
            "core/only-rest",
            raw.clone(),
            &Caller::new(Some(1), "rest", identity.clone())
        )
        .is_ok()
    );
    assert!(
        d2.call(
            "core/only-rest",
            raw,
            &Caller::new(None, INTERNAL_TRANSPORT, identity)
        )
        .is_ok()
    );
}

#[test]
fn rebinding_replaces_the_handler_and_missing_handler_is_internal() {
    let c = Rc::new(Container::new());
    let d = Dispatcher::new(c.clone(), closed_registries());
    assert_eq!(
        d.call("core/get-post", input(1), &caller(Some(1)))
            .unwrap_err()
            .code,
        "operation_no_handler"
    );
    bind_handler(&c, "core/get-post", |_, _| {
        Ok(handler_fn(|_: GetPost, _: &Caller| {
            Ok(Post {
                title: "second".into(),
            })
        }))
    });
    assert_eq!(
        d.call("core/get-post", input(1), &caller(Some(1)))
            .unwrap()
            .get("title"),
        Some(&Value::from("second"))
    );
}

#[test]
fn open_registries_refuse_calls() {
    let d = Dispatcher::new(Rc::new(Container::new()), Rc::new(Registries::new()));
    assert_eq!(
        d.call("core/get-post", input(1), &caller(Some(1)))
            .unwrap_err()
            .code,
        "registry_open"
    );
}
