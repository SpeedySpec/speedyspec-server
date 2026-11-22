//! Fixtures shared by the specification tests.
#![allow(dead_code)]

use std::rc::Rc;

use speedyspec_operation::identity::MemoryIdentity;
use speedyspec_operation::{
    InputMapper, Operation, OperationBuilder, OutputMapper, Registries, S, Type,
};
use speedyspec_operation_contracts::*;

#[derive(Debug, PartialEq)]
pub struct GetPost {
    pub id: i64,
}

impl Hydrate for GetPost {
    fn hydrate(v: &Value) -> Result<Self, OperationError> {
        match v.get("id") {
            Some(Value::Int(id)) => Ok(GetPost { id: *id }),
            _ => Err(OperationError::new("bad", Category::InvalidArgument, "id")),
        }
    }
}

pub struct Post {
    pub title: String,
}

impl Extract for Post {
    fn extract(&self) -> Value {
        Value::map([("title", Value::from(self.title.as_str()))])
    }
}

pub fn get_post() -> OperationBuilder {
    Operation::query("core/get-post")
        .resource("post", Action::Get, &["id"])
        .input::<GetPost>(S::object([("id", S::integer().required())]))
        .output::<Post>(Type::reference("post"))
        .error("post_not_found", Category::NotFound)
        .permission(|c: &Caller, i: &GetPost| c.can("read_post", &[Value::Int(i.id)]))
}

pub fn get_user() -> OperationBuilder {
    Operation::query("core/get-user")
        .input::<GetPost>(S::object([("id", S::integer().required())]))
        .output::<Post>(Type::reference("user"))
        .public()
}

fn close(r: Registries) -> Rc<Registries> {
    r.close().unwrap();
    Rc::new(r)
}

/// Post and user types, `core/get-post` and `core/get-user`, closed.
pub fn closed_registries() -> Rc<Registries> {
    let r = Registries::new();
    r.register_type(
        Type::resource("post")
            .field("title", S::string())
            .field("author_id", S::integer())
            .relation("author", "core/get-user", [("id", "author_id")])
            .build(),
    )
    .unwrap();
    r.register_type(Type::resource("user").field("id", S::integer()).build())
        .unwrap();
    r.register_operation(get_post().build().unwrap()).unwrap();
    r.register_operation(get_user().build().unwrap()).unwrap();
    close(r)
}

/// `core/only-rest`, restricted to REST, with raw contracts.
pub fn restricted_registries() -> Rc<Registries> {
    let r = Registries::new();
    r.register_operation(
        Operation::query("core/only-rest")
            .input_with(S::any(), Rc::new(InputMapper::<RawInput>::new()))
            .output_with(S::any(), Rc::new(OutputMapper::<RawOutput>::new()))
            .public()
            .transports(&["rest"])
            .build()
            .unwrap(),
    )
    .unwrap();
    close(r)
}

/// `core/get-titled`, whose output title must not be empty.
pub fn titled_registries() -> Rc<Registries> {
    let r = Registries::new();
    r.register_operation(
        Operation::query("core/get-titled")
            .input_with(S::any(), Rc::new(InputMapper::<RawInput>::new()))
            .output::<Post>(S::object([("title", S::string().min_length(1))]))
            .public()
            .build()
            .unwrap(),
    )
    .unwrap();
    close(r)
}

/// A REST caller; user 1 may read posts.
pub fn caller(user: Option<u64>) -> Caller {
    Caller::new(
        user,
        "rest",
        Rc::new(MemoryIdentity::new().grant(1, "read_post")),
    )
}
