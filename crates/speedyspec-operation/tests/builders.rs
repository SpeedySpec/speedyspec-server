//! The Type and Operation builders.

use std::rc::Rc;

use speedyspec_operation::identity::MemoryIdentity;
use speedyspec_operation::{Operation, OperationBuilder, RegistrationError, S, Type};
use speedyspec_operation_contracts::*;

#[derive(Debug, PartialEq)]
struct GetPost {
    id: i64,
}

impl Hydrate for GetPost {
    fn hydrate(v: &Value) -> Result<Self, OperationError> {
        match v.get("id") {
            Some(Value::Int(id)) => Ok(GetPost { id: *id }),
            _ => Err(OperationError::new("bad", Category::InvalidArgument, "id")),
        }
    }
}

struct Post {
    title: String,
}

impl Extract for Post {
    fn extract(&self) -> Value {
        Value::map([("title", Value::from(self.title.as_str()))])
    }
}

fn get_post() -> OperationBuilder {
    Operation::query("core/get-post")
        .resource("post", Action::Get, &["id"])
        .input::<GetPost>(S::object([("id", S::integer().required())]))
        .output::<Post>(Type::reference("post"))
        .error("post_not_found", Category::NotFound)
        .permission(|c: &Caller, i: &GetPost| c.can("read_post", &[Value::Int(i.id)]))
}

#[test]
fn builds_a_query_with_its_traits() {
    let spec = get_post().transports(&["rest"]).build().unwrap();
    assert_eq!(spec.kind, Kind::Query);
    assert_eq!(spec.resource.as_ref().unwrap().identity, ["id"]);
    assert!(
        spec.exposed_on("rest")
            && spec.exposed_on(INTERNAL_TRANSPORT)
            && !spec.exposed_on("xmlrpc")
    );
    assert!(get_post().build().unwrap().exposed_on("graphql"));
}

#[test]
fn permission_splits_unauthenticated_and_forbidden() {
    let spec = get_post().build().unwrap();
    let id: Rc<dyn Identity> = Rc::new(MemoryIdentity::new().grant(2, "edit_posts"));
    let input = GetPost { id: 7 };
    let anon = spec
        .permission
        .check(&Caller::new(None, "rest", id.clone()), &input)
        .unwrap_err();
    assert_eq!(
        (anon.code.as_str(), anon.category),
        ("operation_unauthenticated", Category::Unauthenticated)
    );
    let user = spec
        .permission
        .check(&Caller::new(Some(2), "rest", id), &input)
        .unwrap_err();
    assert_eq!(
        (user.code.as_str(), user.category),
        ("operation_forbidden", Category::PermissionDenied)
    );
}

#[test]
fn build_rejects_incomplete_or_inconsistent_operations() {
    assert_eq!(
        Operation::query("Bad").public().build().unwrap_err(),
        RegistrationError::InvalidName { name: "Bad".into() }
    );
    assert!(matches!(
        Operation::query("a/b")
            .public()
            .output::<Post>(S::any())
            .build()
            .unwrap_err(),
        RegistrationError::MissingContract { which: "input", .. }
    ));
    assert!(matches!(
        get_post()
            .error("post_not_found", Category::Conflict)
            .build()
            .unwrap_err(),
        RegistrationError::DuplicateErrorCode { .. }
    ));
    assert!(matches!(
        get_post().destructive().build().unwrap_err(),
        RegistrationError::DestructiveQuery { .. }
    ));
    let no_perm = Operation::query("a/b")
        .input::<GetPost>(S::any())
        .output::<Post>(S::any());
    assert!(matches!(
        no_perm.build().unwrap_err(),
        RegistrationError::MissingPermission { .. }
    ));
}

#[test]
fn type_builder_collects_fields_and_relations() {
    let t = Type::resource("post")
        .field("author_id", S::integer())
        .relation("author", "core/get-user", [("id", "author_id")])
        .build();
    assert_eq!(
        t.relations[0].args.get("id").map(String::as_str),
        Some("author_id")
    );
}
