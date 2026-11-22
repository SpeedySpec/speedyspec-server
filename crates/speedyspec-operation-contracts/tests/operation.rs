//! Operation names, the caller and the raw contracts.

use std::rc::Rc;

use speedyspec_operation_contracts::*;

struct Grants;

impl Identity for Grants {
    fn can(&self, user: Option<u64>, cap: &str, _: &[Value]) -> bool {
        user == Some(1) && cap == "edit_posts"
    }
}

#[test]
fn names_follow_the_abilities_rule() {
    assert_eq!(
        OperationName::parse("core/get-post").unwrap().as_str(),
        "core/get-post"
    );
    for bad in [
        "core",
        "Core/get",
        "core/get/post",
        "core/get_post",
        "/get",
        "core/",
    ] {
        assert!(OperationName::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn caller_delegates_to_identity() {
    let c = Caller::new(Some(1), "rest", Rc::new(Grants));
    assert!(c.can("edit_posts", &[]));
    assert!(!Caller::new(None, "rest", Rc::new(Grants)).can("edit_posts", &[]));
}

#[test]
fn raw_input_and_output_round_trip_through_the_neutral_value() {
    let mut input = RawInput {
        args: Value::map([("id", Value::from(3i64))]),
        method: Some("GET".into()),
        ..Default::default()
    };
    input.headers.insert("x-wp-nonce".into(), "abc".into());
    assert_eq!(RawInput::hydrate(&input.extract()).unwrap(), input);
    let output = RawOutput {
        value: Value::from("ok"),
        status: Some(201),
        ..Default::default()
    };
    assert_eq!(output.extract().get("status"), Some(&Value::Int(201)));
    assert_eq!(RawOutput::hydrate(&output.extract()).unwrap(), output);
}

#[test]
fn raw_input_rejects_a_non_object() {
    let e = RawInput::hydrate(&Value::from(1i64)).unwrap_err();
    assert_eq!(e.category, Category::InvalidArgument);
}
