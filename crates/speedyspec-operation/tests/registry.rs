//! Registries: close-time checks, re-registration and the exported document.

mod common;

use common::*;
use speedyspec_operation::{RegistrationError, Registries, S, Type};
use speedyspec_operation_contracts::*;

fn post_type() -> TypeSpec {
    Type::resource("post")
        .field("title", S::string())
        .field("author_id", S::integer())
        .relation("author", "core/get-user", [("id", "author_id")])
        .build()
}

fn user_type() -> TypeSpec {
    Type::resource("user")
        .field("id", S::integer())
        .relation("posts", "core/get-post", [("id", "id")])
        .build()
}

#[test]
fn closes_with_relation_cycles_and_exports_a_document() {
    let r = Registries::new();
    r.register_type(post_type()).unwrap();
    r.register_type(user_type()).unwrap();
    r.register_operation(get_post().build().unwrap()).unwrap();
    r.register_operation(get_user().build().unwrap()).unwrap();
    r.close().unwrap();
    let doc = r.document();
    assert_eq!(
        doc["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(doc["operations"]["core/get-post"]["kind"], "query");
    assert_eq!(
        doc["operations"]["core/get-post"]["resource"]["action"],
        "get"
    );
    assert_eq!(
        doc["operations"]["core/get-post"]["output"],
        serde_json::json!({"$ref": "#/types/post"})
    );
    assert_eq!(
        doc["operations"]["core/get-post"]["transports"],
        serde_json::Value::Null
    );
    assert_eq!(
        doc["types"]["post"]["relations"][0]["operation"],
        "core/get-user"
    );
}

#[test]
fn close_collects_every_dangling_reference() {
    let r = Registries::new();
    r.register_type(post_type()).unwrap(); // core/get-user missing
    r.register_operation(get_post().build().unwrap()).unwrap();
    let errs = r.close().unwrap_err();
    assert!(errs.contains(&RegistrationError::UnknownOperation {
        at: "post.author".into(),
        operation: "core/get-user".into()
    }));
    assert!(!r.is_closed());
}

#[test]
fn relation_arg_and_source_must_exist() {
    let r = Registries::new();
    r.register_type(
        Type::resource("post")
            .field("author_id", S::integer())
            .relation("author", "core/get-user", [("uid", "writer")])
            .build(),
    )
    .unwrap();
    r.register_type(user_type()).unwrap();
    r.register_operation(get_post().build().unwrap()).unwrap();
    r.register_operation(get_user().build().unwrap()).unwrap();
    let errs = r.close().unwrap_err();
    assert!(
        errs.iter().any(
            |e| matches!(e, RegistrationError::RelationArgMissing { arg, .. } if arg == "uid")
        )
    );
    assert!(
        errs.iter()
            .any(|e| matches!(e, RegistrationError::RelationSourceMissing { source, .. } if source == "writer"))
    );
}

#[test]
fn duplicates_before_close_and_reregistration_after_close() {
    let r = Registries::new();
    r.register_type(post_type()).unwrap();
    r.register_type(user_type()).unwrap();
    r.register_operation(get_post().build().unwrap()).unwrap();
    r.register_operation(get_user().build().unwrap()).unwrap();
    assert!(matches!(
        r.register_operation(get_post().build().unwrap()),
        Err(RegistrationError::Duplicate { .. })
    ));
    r.close().unwrap();
    assert_eq!(r.register_operation(get_post().build().unwrap()), Ok(()));
    assert_eq!(
        r.register_operation(get_post().transports(&["rest"]).build().unwrap()),
        Err(RegistrationError::Closed {
            name: "core/get-post".into()
        })
    );
}

#[test]
fn invalid_pattern_fails_at_registration() {
    let r = Registries::new();
    let bad = Type::resource("t")
        .field("slug", S::string().pattern("("))
        .build();
    assert!(
        matches!(r.register_type(bad), Err(RegistrationError::InvalidSchema { at, .. }) if at == "t.slug")
    );
}
