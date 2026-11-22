//! The serve pipeline, driven by a toy "line" transport: `"<user|-> <operation> <id>"` in, `"ok <title>"` or
//! `"err <code>"` out.

mod common;

use std::rc::Rc;

use common::*;
use speedyspec_container::Container;
use speedyspec_operation::identity::MemoryIdentity;
use speedyspec_operation::{Registries, S, Type};
use speedyspec_operation_contracts::*;
use speedyspec_operation_dispatch::*;
use speedyspec_transport::*;
use speedyspec_transport_contracts::*;

struct LineAuth {
    label: &'static str,
}

impl Authenticator<String> for LineAuth {
    fn authenticate(&self, request: &String) -> Result<Caller, OperationError> {
        let identity = Rc::new(MemoryIdentity::new().grant(1, "read_post"));
        match request.split(' ').next() {
            Some("-") => Ok(Caller::new(None, self.label, identity)),
            Some(word) => word
                .parse()
                .map(|user| Caller::new(Some(user), self.label, identity))
                .map_err(|_| {
                    OperationError::new("bad_credentials", Category::Unauthenticated, "who?")
                }),
            None => Err(OperationError::new(
                "bad_credentials",
                Category::Unauthenticated,
                "who?",
            )),
        }
    }
}

struct LineCodec;

impl Codec<String, String> for LineCodec {
    fn decode(
        &self,
        request: &String,
        _: &dyn ExposureLookup,
    ) -> Result<DecodedCall, OperationError> {
        let parts: Vec<&str> = request.split(' ').collect();
        let id: i64 = parts
            .get(2)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| OperationError::new("bad_request", Category::InvalidArgument, "id"))?;
        Ok(DecodedCall {
            operation: parts[1].to_owned(),
            input: Value::map([("id", Value::Int(id))]),
            requested_fields: None,
        })
    }

    fn encode(&self, _: &String, _: &DecodedCall, output: Value) -> String {
        match output.get("title") {
            Some(Value::String(t)) => format!("ok {t}"),
            _ => "ok".into(),
        }
    }

    fn encode_error(&self, _: &String, error: &OperationError) -> String {
        format!("err {}", error.code)
    }
}

fn build(restriction: Option<&[&str]>, label: &'static str) -> Pipeline<String, String> {
    let registries = Registries::new();
    registries
        .register_type(
            Type::resource("post")
                .field("title", S::string())
                .field("author_id", S::integer())
                .build(),
        )
        .unwrap();
    let mut spec = get_post();
    if let Some(names) = restriction {
        spec = spec.transports(names);
    }
    registries
        .register_operation(spec.build().unwrap())
        .unwrap();
    registries.close().unwrap();
    let registries = Rc::new(registries);

    let container = Rc::new(Container::new());
    bind_handler(&container, "core/get-post", |_, _| {
        Ok(handler_fn(|i: GetPost, _: &Caller| {
            Ok(Post {
                title: format!("post {}", i.id),
            })
        }))
    });
    let dispatcher = Rc::new(Dispatcher::new(container, registries.clone()));
    let transports = TransportRegistry::new(Rc::new(MemoryBindingStore::new()));
    transports
        .install(Rc::new(FakeTransport::new("line", true)))
        .unwrap();
    let exposure = Rc::new(transports.expose(&registries).unwrap());
    Pipeline::new(
        "line",
        Rc::new(LineAuth { label }),
        Rc::new(LineCodec),
        dispatcher,
        exposure,
    )
}

fn pipeline() -> Pipeline<String, String> {
    build(None, "line")
}

#[test]
fn serves_a_request_end_to_end() {
    assert_eq!(
        pipeline().serve(&"1 core/get-post 7".to_owned()),
        "ok post 7"
    );
}

#[test]
fn every_stage_failure_is_encoded_as_an_error() {
    let p = pipeline();
    assert_eq!(
        p.serve(&"x core/get-post 7".to_owned()),
        "err bad_credentials"
    );
    assert_eq!(
        p.serve(&"1 core/get-post seven".to_owned()),
        "err bad_request"
    );
    assert_eq!(
        p.serve(&"- core/get-post 7".to_owned()),
        "err operation_unauthenticated"
    );
    assert_eq!(
        p.serve(&"1 core/nope 7".to_owned()),
        "err operation_not_found"
    );
}

#[test]
fn operations_the_transport_does_not_expose_are_not_found() {
    assert_eq!(
        build(Some(&["rest"]), "line").serve(&"1 core/get-post 7".to_owned()),
        "err operation_not_found"
    );
}

#[test]
fn the_pipeline_owns_the_caller_transport() {
    // The authenticator labels callers "rest"; the pipeline relabels them "line", so a line-only operation is reached.
    // Without relabelling, the dispatcher would see a "rest" caller and hide the operation.
    assert_eq!(
        build(Some(&["line"]), "rest").serve(&"1 core/get-post 7".to_owned()),
        "ok post 7"
    );
}
