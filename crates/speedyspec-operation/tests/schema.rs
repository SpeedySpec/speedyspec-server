//! The S builder, JSON Schema rendering and S::raw.

use serde_json::json;
use speedyspec_operation::{S, SchemaError, export::json_schema};
use speedyspec_operation_contracts::{SchemaKind, StringFormat};

#[test]
fn object_renders_properties_required_and_constraints() {
    let s = S::object([
        ("id", S::integer().required()),
        ("title", S::string().max_length(200)),
        ("file", S::binary("application/pdf")),
        ("when", S::date_time().nullable()),
    ])
    .build();
    assert_eq!(
        json_schema(&s),
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "integer"},
                "title": {"type": "string", "maxLength": 200},
                "file": {"type": "string", "contentEncoding": "base64", "contentMediaType": "application/pdf"},
                "when": {"type": ["string", "null"], "format": "date-time"}
            },
            "required": ["id"]
        })
    );
}

#[test]
fn closed_object_and_reference_and_one_of() {
    assert_eq!(
        json_schema(&S::object([("a", S::boolean())]).closed().build())["additionalProperties"],
        json!(false)
    );
    assert_eq!(
        json_schema(&S::reference("post").build()),
        json!({"$ref": "#/types/post"})
    );
    let one = S::one_of([S::reference("post"), S::reference("page")])
        .discriminator("type")
        .build();
    assert_eq!(
        json_schema(&one),
        json!({
            "oneOf": [{"$ref": "#/types/post"}, {"$ref": "#/types/page"}],
            "discriminator": {"propertyName": "type"}
        })
    );
}

#[test]
fn raw_round_trips_what_json_schema_emits() {
    let s = S::object([("email", S::string().format(StringFormat::Email).required())]).build();
    assert_eq!(S::raw(&json_schema(&s)).unwrap().build(), s);
    assert_eq!(S::raw(&json!({})).unwrap().build().kind, SchemaKind::Any);
}

#[test]
fn raw_rejects_keywords_it_does_not_support() {
    assert_eq!(
        S::raw(&json!({"type": "string", "if": {}})).unwrap_err(),
        SchemaError::Unsupported {
            keyword: "if".into()
        }
    );
}

#[test]
#[should_panic(expected = "max_length applies to string schemas")]
fn constraint_on_wrong_kind_panics() {
    let _ = S::integer().max_length(3);
}

#[test]
fn from_value_keeps_property_order() {
    use speedyspec_operation_contracts::Value;
    let int = Value::map([("type", Value::from("integer"))]);
    let schema = S::from_value(&Value::map([
        ("type", Value::from("object")),
        (
            "properties",
            Value::map([("z", int.clone()), ("a", int.clone()), ("m", int)]),
        ),
    ]))
    .unwrap()
    .build();
    let SchemaKind::Object { properties, .. } = &schema.kind else {
        panic!("not an object: {schema:?}");
    };
    assert_eq!(properties.keys().collect::<Vec<_>>(), ["z", "a", "m"]);
}

#[test]
fn from_value_reads_an_empty_list_as_any() {
    use speedyspec_operation_contracts::Value;
    // PHP writes an empty schema object, `S::any()`, as `[]`.
    let schema = S::from_value(&Value::List(vec![])).unwrap().build();
    assert_eq!(schema.kind, SchemaKind::Any);
}
