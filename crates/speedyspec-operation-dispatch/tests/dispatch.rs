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

#[test]
fn integral_float_ids_reach_the_handler_as_integers() {
    let out = dispatcher(ok_handler())
        .call(
            "core/get-post",
            Value::map([("id", Value::Float(42.0))]),
            &caller(Some(1)),
        )
        .unwrap();
    assert_eq!(out.get("title"), Some(&Value::from("post 42")));
}

struct Short;

impl OperationMiddleware for Short {
    fn around(
        &self,
        call: &Call<'_>,
        _: &dyn Fn() -> Result<Box<dyn std::any::Any>, OperationError>,
    ) -> Result<Box<dyn std::any::Any>, OperationError> {
        Ok(Box::new(Bypass(Value::map([("seen", call.raw.clone())]))))
    }
}

#[test]
fn middleware_sees_the_normalized_input_and_can_bypass_the_handler() {
    let ran = Rc::new(RefCell::new(false));
    let seen = ran.clone();
    let d = dispatcher(handler_fn(move |_: GetPost, _: &Caller| {
        *seen.borrow_mut() = true;
        Ok(Post { title: "x".into() })
    }))
    .with_middleware(Rc::new(Short));
    let out = d
        .call(
            "core/get-post",
            Value::map([("id", Value::Float(5.0))]),
            &caller(Some(1)),
        )
        .unwrap();
    assert_eq!(
        out,
        Value::map([("seen", Value::map([("id", Value::Int(5))]))])
    );
    assert!(!*ran.borrow());
}

#[test]
fn bypass_still_runs_after_the_permission_check() {
    let d = dispatcher(ok_handler()).with_middleware(Rc::new(Short));
    assert_eq!(
        d.call("core/get-post", input(1), &caller(None))
            .unwrap_err()
            .category,
        Category::Unauthenticated
    );
}

#[test]
fn output_is_normalized_before_validation() {
    use speedyspec_operation::{Operation, S};
    struct When;
    impl Extract for When {
        fn extract(&self) -> Value {
            Value::map([("when", Value::from("2026-11-22T10:00:00"))])
        }
    }
    let r = Registries::new();
    r.register_operation(
        Operation::query("a/when")
            .input::<RawInput>(S::object([]))
            .output::<When>(S::object([("when", S::date_time().required())]))
            .public()
            .build()
            .unwrap(),
    )
    .unwrap();
    r.close().unwrap();
    let c = Rc::new(Container::new());
    let h = handler_fn(|_: RawInput, _: &Caller| Ok(When));
    bind_handler(&c, "a/when", move |_, _| Ok(h.clone()));
    let out = Dispatcher::new(c, Rc::new(r))
        .call("a/when", Value::map::<&str>([]), &caller(None))
        .unwrap();
    let Some(Value::DateTime(dt)) = out.get("when") else {
        panic!("not a date time: {out:?}");
    };
    assert_eq!(dt.to_string(), "2026-11-22 10:00:00");
}

struct Neutral {
    ran: Rc<RefCell<bool>>,
    output: Value,
}

impl OperationHandler for Neutral {
    fn handle(
        &self,
        _: Box<dyn std::any::Any>,
        _: &Caller,
    ) -> Result<Box<dyn std::any::Any>, OperationError> {
        panic!("a neutral handler is called through handle_neutral");
    }

    fn neutral(&self) -> bool {
        true
    }

    fn handle_neutral(&self, input: &Value, _: &Caller) -> Result<Value, OperationError> {
        *self.ran.borrow_mut() = true;
        match (&self.output, input.get("id")) {
            (Value::Null, Some(Value::Int(id))) => Ok(Value::map([(
                "title",
                Value::from(format!("n {id}").as_str()),
            )])),
            (output, _) => Ok(output.clone()),
        }
    }
}

fn neutral(output: Value) -> (Rc<dyn OperationHandler>, Rc<RefCell<bool>>) {
    let ran = Rc::new(RefCell::new(false));
    (
        Rc::new(Neutral {
            ran: ran.clone(),
            output,
        }),
        ran,
    )
}

#[test]
fn neutral_handler_takes_and_returns_values() {
    let (h, ran) = neutral(Value::Null);
    let out = dispatcher(h)
        .call("core/get-post", input(7), &caller(Some(1)))
        .unwrap();
    assert_eq!(out, Value::map([("title", Value::from("n 7"))]));
    assert!(*ran.borrow());
}

#[test]
fn neutral_output_is_still_validated() {
    let (h, _) = neutral(Value::map([("title", Value::Int(5))]));
    let e = dispatcher(h)
        .call("core/get-post", input(7), &caller(Some(1)))
        .unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.category),
        ("invalid_output", Category::Internal)
    );
}

#[test]
fn neutral_handler_still_passes_permission() {
    let (h, ran) = neutral(Value::Null);
    let e = dispatcher(h)
        .call("core/get-post", input(7), &caller(Some(2)))
        .unwrap_err();
    assert_eq!(e.category, Category::PermissionDenied);
    assert!(!*ran.borrow());
}

#[test]
fn validate_and_authorize_are_separate_steps() {
    let d = dispatcher(ok_handler());
    assert_eq!(d.validate("core/get-post", &input(7)), Ok(()));
    assert_eq!(
        d.validate("core/get-post", &Value::map::<&str>([]))
            .map_err(|e| e.code),
        Err("invalid_input".to_owned())
    );
    assert_eq!(
        d.authorize("core/get-post", &input(7), &caller(Some(1))),
        Ok(())
    );
    let refused = d
        .authorize("core/get-post", &input(7), &caller(Some(2)))
        .unwrap_err();
    assert!(matches!(
        refused.category,
        Category::PermissionDenied | Category::Unauthenticated
    ));
    assert_eq!(
        d.validate("core/nothing", &input(1)).map_err(|e| e.code),
        Err("operation_not_found".to_owned())
    );
}
