//! Fixtures shared by the specification tests.
#![allow(dead_code)]

use speedyspec_operation::{Operation, OperationBuilder, S, Type};
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
