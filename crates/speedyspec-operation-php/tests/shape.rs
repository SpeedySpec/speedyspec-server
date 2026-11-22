//! The schema-versus-class registration check.

use std::rc::Rc;

use speedyspec_operation::{S, SchemaBuilder, Type};
use speedyspec_operation_contracts::{TypeCatalog, TypeSpec};
use speedyspec_operation_php::shape::{ClassShape, Member, Side, check_shape};
use speedyspec_php_ext::{Key, Value as PhpValue};

struct Types(Vec<Rc<TypeSpec>>);

impl TypeCatalog for Types {
    fn get_type(&self, name: &str) -> Option<Rc<TypeSpec>> {
        self.0.iter().find(|t| t.name == name).cloned()
    }
    fn type_names(&self) -> Vec<String> {
        self.0.iter().map(|t| t.name.clone()).collect()
    }
}

fn none() -> Types {
    Types(vec![])
}

fn member(name: &str, types: &[&str], has_default: bool) -> Member {
    Member {
        name: name.into(),
        types: types.iter().map(|t| (*t).to_owned()).collect(),
        nullable: types.contains(&"null") || types.is_empty() || types.contains(&"mixed"),
        has_default,
    }
}

fn shape(class: &str, members: Vec<Member>) -> ClassShape {
    ClassShape {
        class: class.into(),
        members,
    }
}

fn check(schema: SchemaBuilder, shape: &ClassShape, side: Side) -> Vec<String> {
    check_shape(&schema.build(), shape, side, &none())
}

fn report() -> SchemaBuilder {
    S::object([("id", S::integer().required()), ("title", S::string())])
}

#[test]
fn matching_input_class_passes() {
    let s = shape(
        "Acme\\In",
        vec![
            member("id", &["int"], false),
            member("title", &["string", "null"], true),
        ],
    );
    assert_eq!(check(report(), &s, Side::Input), Vec::<String>::new());
}

#[test]
fn type_mismatch_is_reported() {
    let s = shape(
        "Acme\\In",
        vec![
            member("id", &["string"], false),
            member("title", &["string", "null"], true),
        ],
    );
    assert_eq!(
        check(report(), &s, Side::Input),
        ["Acme\\In::$id is string, the schema says integer"]
    );
}

#[test]
fn optional_input_needs_null_or_default() {
    let s = shape(
        "Acme\\In",
        vec![
            member("id", &["int"], false),
            member("title", &["string"], false),
        ],
    );
    assert_eq!(
        check(report(), &s, Side::Input),
        ["Acme\\In::$title is optional in the schema but has no default and is not nullable"]
    );
}

#[test]
fn nullable_schema_needs_nullable_member() {
    let s = shape("Acme\\In", vec![member("title", &["string"], true)]);
    assert_eq!(
        check(
            S::object([("title", S::string().nullable())]),
            &s,
            Side::Input
        ),
        ["Acme\\In::$title may be null in the schema but its type does not allow null"]
    );
}

#[test]
fn schema_property_missing_from_class() {
    let input = shape("Acme\\In", vec![member("id", &["int"], false)]);
    assert_eq!(
        check(report(), &input, Side::Input),
        ["Acme\\In has no constructor parameter $title"]
    );
    let output = shape("Acme\\Out", vec![member("id", &["int"], false)]);
    assert_eq!(
        check(
            S::object([
                ("id", S::integer().required()),
                ("title", S::string().required())
            ]),
            &output,
            Side::Output
        ),
        ["Acme\\Out has no public property $title"]
    );
    assert_eq!(check(report(), &output, Side::Output), Vec::<String>::new());
}

#[test]
fn extra_input_param_without_default() {
    let s = shape(
        "Acme\\In",
        vec![
            member("id", &["int"], false),
            member("title", &["string", "null"], true),
            member("extra", &["int"], false),
        ],
    );
    assert_eq!(
        check(report(), &s, Side::Input),
        ["Acme\\In::$extra is not in the schema and has no default"]
    );
}

#[test]
fn extra_output_property() {
    let s = shape(
        "Acme\\Out",
        vec![
            member("id", &["int"], false),
            member("title", &["string"], false),
            member("extra", &["int"], false),
        ],
    );
    assert_eq!(check(report(), &s, Side::Output), Vec::<String>::new());
    assert_eq!(
        check(report().closed(), &s, Side::Output),
        ["Acme\\Out::$extra is not in the closed schema"]
    );
}

#[test]
fn accepted_type_table() {
    let rows: Vec<(SchemaBuilder, &[&str])> = vec![
        (S::boolean(), &["bool", "true", "false"]),
        (S::integer(), &["int"]),
        (S::number(), &["float", "int"]),
        (S::string(), &["string"]),
        (
            S::date_time(),
            &[
                "datetimeinterface",
                "datetimeimmutable",
                "datetime",
                "string",
            ],
        ),
        (S::binary_any(), &["string"]),
        (S::array(S::any()), &["array", "iterable"]),
        (S::object([]), &["array", "object", "acme\\thing"]),
        (S::reference("post"), &["array", "object", "acme\\post"]),
        (
            S::one_of([S::string(), S::integer()]),
            &["array", "object", "acme\\x"],
        ),
        (S::any(), &["int", "string", "array", "acme\\x"]),
    ];
    for (schema, accepted) in rows {
        let schema = schema.required();
        for php in accepted {
            let s = shape("C", vec![member("v", &[php], false)]);
            assert_eq!(
                check(S::object([("v", schema.clone())]), &s, Side::Input),
                Vec::<String>::new(),
                "{php} should satisfy {:?}",
                schema.clone().build().kind
            );
        }
    }
    let untyped = shape("C", vec![member("v", &[], false)]);
    assert_eq!(
        check(
            S::object([("v", S::integer().required())]),
            &untyped,
            Side::Input
        ),
        Vec::<String>::new()
    );
    let wrong = shape("C", vec![member("v", &["array"], false)]);
    assert_eq!(
        check(
            S::object([("v", S::boolean().required())]),
            &wrong,
            Side::Input
        ),
        ["C::$v is array, the schema says boolean"]
    );
}

#[test]
fn ref_schema_uses_the_type_fields() {
    let types = Types(vec![Rc::new(
        Type::resource("report").field("id", S::integer()).build(),
    )]);
    let s = shape("Acme\\Report", vec![member("id", &["string"], false)]);
    assert_eq!(
        check_shape(&S::reference("report").build(), &s, Side::Output, &types),
        ["Acme\\Report::$id is string, the schema says integer"]
    );
}

#[test]
fn ref_to_unregistered_type_is_skipped() {
    let s = shape("Acme\\Report", vec![member("id", &["string"], false)]);
    assert_eq!(
        check(S::reference("report"), &s, Side::Output),
        Vec::<String>::new()
    );
}

#[test]
fn class_needs_an_object_schema() {
    let s = shape("Acme\\In", vec![]);
    assert_eq!(
        check(S::integer(), &s, Side::Input),
        ["Acme\\In needs an object schema, the schema is integer"]
    );
}

#[test]
fn shape_reads_the_php_array() {
    let php = PhpValue::Array(vec![
        (Key::Str("class".into()), PhpValue::Str("Acme\\In".into())),
        (
            Key::Str("members".into()),
            PhpValue::Array(vec![(
                Key::Int(0),
                PhpValue::Array(vec![
                    (Key::Str("name".into()), PhpValue::Str("id".into())),
                    (
                        Key::Str("types".into()),
                        PhpValue::Array(vec![(Key::Int(0), PhpValue::Str("int".into()))]),
                    ),
                    (Key::Str("nullable".into()), PhpValue::Bool(false)),
                    (Key::Str("default".into()), PhpValue::Bool(true)),
                ]),
            )]),
        ),
    ]);
    assert_eq!(
        ClassShape::from_php(&php).unwrap(),
        shape(
            "Acme\\In",
            vec![Member {
                name: "id".into(),
                types: vec!["int".into()],
                nullable: false,
                has_default: true
            }]
        )
    );
    assert!(ClassShape::from_php(&PhpValue::Int(1)).is_err());
}
