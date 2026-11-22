//! Stage middleware: before-handler hooks per transport.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::*;
use speedyspec_container::Container;
use speedyspec_operation_contracts::*;
use speedyspec_operation_dispatch::*;
use speedyspec_transport::StageMiddleware;
use speedyspec_transport_contracts::{StageHooks, TransportHooks};

#[derive(Default)]
struct RecordingHooks {
    filters: RefCell<Vec<(String, Vec<Value>)>>,
}

impl RecordingHooks {
    fn filters(&self) -> Vec<String> {
        self.filters
            .borrow()
            .iter()
            .map(|(h, _)| h.clone())
            .collect()
    }

    fn filter_args(&self, hook: &str) -> Vec<Value> {
        self.filters
            .borrow()
            .iter()
            .filter(|(h, _)| h == hook)
            .flat_map(|(_, args)| args.clone())
            .collect()
    }
}

impl TransportHooks for RecordingHooks {
    fn action(&self, _: &str, _: Vec<Value>) -> Result<(), OperationError> {
        Ok(())
    }

    fn filter(&self, hook: &str, value: Value, args: Vec<Value>) -> Result<Value, OperationError> {
        self.filters.borrow_mut().push((hook.to_owned(), args));
        Ok(if hook == "short" {
            Value::from("short-circuited")
        } else {
            value
        })
    }
}

struct NamedStage(&'static str);

impl StageHooks for NamedStage {
    fn before_handler(
        &self,
        _: &OperationSpec,
        _: &Caller,
        input: &Value,
        hooks: &dyn TransportHooks,
    ) -> Result<Option<Value>, OperationError> {
        let v = hooks.filter(self.0, Value::Null, vec![input.clone()])?;
        Ok((v != Value::Null).then_some(v))
    }
}

fn dispatcher(stage: StageMiddleware) -> Dispatcher {
    let c = Rc::new(Container::new());
    bind_handler(&c, "core/get-post", |_, _| {
        Ok(handler_fn(|i: GetPost, _: &Caller| {
            Ok(Post {
                title: format!("post {}", i.id),
            })
        }))
    });
    Dispatcher::new(c, closed_registries()).with_middleware(Rc::new(stage))
}

fn input(id: i64) -> Value {
    Value::map([("id", Value::from(id))])
}

#[test]
fn a_stage_hook_can_short_circuit_after_the_permission_check() {
    let hooks = Rc::new(RecordingHooks::default());
    let d =
        dispatcher(StageMiddleware::new(hooks.clone()).stage("rest", Rc::new(NamedStage("short"))));
    assert_eq!(
        d.call("core/get-post", input(1), &caller(Some(1))).unwrap(),
        Value::from("short-circuited")
    );
    assert_eq!(
        d.call("core/get-post", input(1), &caller(None))
            .unwrap_err()
            .category,
        Category::Unauthenticated
    );
    assert_eq!(hooks.filters(), ["short"]); // the refused call never reached the stage
}

#[test]
fn a_stage_that_returns_none_lets_the_handler_run_with_the_neutral_input() {
    let hooks = Rc::new(RecordingHooks::default());
    let d = dispatcher(
        StageMiddleware::new(hooks.clone()).stage("rest", Rc::new(NamedStage("observe"))),
    );
    assert_eq!(
        d.call("core/get-post", input(3), &caller(Some(1)))
            .unwrap()
            .get("title"),
        Some(&Value::from("post 3"))
    );
    assert_eq!(
        hooks.filter_args("observe"),
        vec![Value::map([("id", Value::Int(3))])]
    );
}

#[test]
fn transports_without_a_stage_pass_through() {
    let hooks = Rc::new(RecordingHooks::default());
    let d = dispatcher(
        StageMiddleware::new(hooks.clone()).stage("xmlrpc", Rc::new(NamedStage("short"))),
    );
    assert!(d.call("core/get-post", input(1), &caller(Some(1))).is_ok()); // caller is on "rest"
    assert!(hooks.filters().is_empty());
}
