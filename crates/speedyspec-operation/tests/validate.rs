//! The schema validator.

use std::rc::Rc;

use speedyspec_operation::S;
use speedyspec_operation::validate::{Violation, validate};
use speedyspec_operation_contracts::{StringFormat, TypeCatalog, TypeSpec, Value};

struct NoTypes;

impl TypeCatalog for NoTypes {
    fn get_type(&self, _: &str) -> Option<Rc<TypeSpec>> {
        None
    }

    fn type_names(&self) -> Vec<String> {
        vec![]
    }
}

fn rules(r: Result<(), Vec<Violation>>) -> Vec<(String, &'static str)> {
    r.err()
        .unwrap_or_default()
        .into_iter()
        .map(|v| (v.path, v.rule))
        .collect()
}

#[test]
fn null_versus_missing() {
    let s = S::object([
        ("id", S::integer().required()),
        ("note", S::string()),
        ("tag", S::string().nullable()),
    ])
    .build();
    assert_eq!(
        rules(validate(&s, &Value::map::<&str>([]), &NoTypes)),
        [("id".into(), "required")]
    );
    assert_eq!(
        rules(validate(
            &s,
            &Value::map([("id", Value::from(1i64)), ("note", Value::Null)]),
            &NoTypes
        )),
        [("note".into(), "type")]
    );
    assert!(
        validate(
            &s,
            &Value::map([("id", Value::from(1i64)), ("tag", Value::Null)]),
            &NoTypes
        )
        .is_ok()
    );
}

#[test]
fn unknown_properties_pass_unless_closed() {
    let open = S::object([("a", S::boolean())]).build();
    let closed = S::object([("a", S::boolean())]).closed().build();
    let v = Value::map([("a", Value::from(true)), ("zzz", Value::from(1i64))]);
    assert!(validate(&open, &v, &NoTypes).is_ok());
    assert_eq!(
        rules(validate(&closed, &v, &NoTypes)),
        [("zzz".into(), "additionalProperties")]
    );
}

#[test]
fn integral_floats_are_integers() {
    assert!(validate(&S::integer().build(), &Value::Float(42.0), &NoTypes).is_ok());
    assert_eq!(
        rules(validate(
            &S::integer().build(),
            &Value::Float(42.5),
            &NoTypes
        )),
        [("".into(), "type")]
    );
    assert!(validate(&S::number().build(), &Value::Int(3), &NoTypes).is_ok());
    assert_eq!(
        rules(validate(
            &S::integer().build(),
            &Value::from("42"),
            &NoTypes
        )),
        [("".into(), "type")]
    );
}

#[test]
fn collects_every_violation_with_paths() {
    let s = S::object([
        ("title", S::string().max_length(3)),
        (
            "tags",
            S::array(S::string().pattern("^[a-z]+$")).max_items(1),
        ),
    ])
    .build();
    let v = Value::map([
        ("title", Value::from("héllo")),
        (
            "tags",
            Value::List(vec![Value::from("ok"), Value::from("Bad")]),
        ),
    ]);
    assert_eq!(
        rules(validate(&s, &v, &NoTypes)),
        [
            ("title".into(), "maxLength"),
            ("tags".into(), "maxItems"),
            ("tags[1]".into(), "pattern"),
        ]
    );
}

#[test]
fn formats_and_enum_and_ref() {
    assert!(
        validate(
            &S::string().format(StringFormat::Email).build(),
            &Value::from("a@b.co"),
            &NoTypes
        )
        .is_ok()
    );
    assert_eq!(
        rules(validate(
            &S::string()
                .enum_values(vec!["draft".into(), "publish".into()])
                .build(),
            &Value::from("trash"),
            &NoTypes
        )),
        [("".into(), "enum")]
    );
    assert_eq!(
        rules(validate(
            &S::reference("post").build(),
            &Value::map::<&str>([]),
            &NoTypes
        )),
        [("".into(), "ref")]
    );
}

#[test]
fn normalize_turns_integral_floats_into_integers_under_integer_schemas() {
    use speedyspec_operation::validate::normalize;
    let s = S::object([
        ("id", S::integer().enum_values(vec![Value::Int(1)])),
        ("ratio", S::number()),
        ("ids", S::array(S::integer())),
    ])
    .build();
    let v = Value::map([
        ("id", Value::Float(1.0)),
        ("ratio", Value::Float(2.0)),
        (
            "ids",
            Value::List(vec![Value::Float(3.0), Value::Float(3.5)]),
        ),
    ]);
    let n = normalize(&s, &v, &NoTypes);
    assert_eq!(n.get("id"), Some(&Value::Int(1)));
    assert_eq!(n.get("ratio"), Some(&Value::Float(2.0)));
    assert_eq!(
        n.get("ids"),
        Some(&Value::List(vec![Value::Int(3), Value::Float(3.5)]))
    );
    assert!(validate(&s, &Value::map([("id", Value::Float(1.0))]), &NoTypes).is_err());
    assert!(
        validate(
            &s,
            &normalize(&s, &Value::map([("id", Value::Float(1.0))]), &NoTypes),
            &NoTypes
        )
        .is_ok()
    );
}
