use speedyspec_operation_contracts::{Category, OperationError, Value};
use speedyspec_xmlrpc_contracts::{Fault, FaultMapping, XmlRpcBinding, XmlRpcParam};

struct Always500;

impl FaultMapping for Always500 {
    fn code(&self, _category: Category) -> i64 {
        500
    }
}

#[test]
fn binding_round_trips_through_value() {
    let binding = XmlRpcBinding {
        method: "acme.editReport".into(),
        params: vec![
            XmlRpcParam::BlogId,
            XmlRpcParam::Username,
            XmlRpcParam::Password,
            XmlRpcParam::Field("id".into()),
            XmlRpcParam::Rest,
        ],
    };
    let value = binding.to_value();
    assert_eq!(
        value.get("params"),
        Some(&Value::List(vec![
            Value::String("blog_id".into()),
            Value::String("username".into()),
            Value::String("password".into()),
            Value::String("id".into()),
            Value::String("*".into()),
        ]))
    );
    assert_eq!(XmlRpcBinding::from_value(&value), Ok(binding));
}

#[test]
fn binding_rejects_system_and_malformed_names() {
    for name in ["system.foo", "foo", "a..b", ".a", "a.", "a.b c"] {
        let value = Value::map([
            ("method", Value::String(name.into())),
            ("params", Value::List(vec![])),
        ]);
        assert!(XmlRpcBinding::from_value(&value).is_err(), "{name}");
    }
    let ok = Value::map([
        ("method", Value::String("wp_x.get_Thing2".into())),
        ("params", Value::List(vec![])),
    ]);
    assert!(XmlRpcBinding::from_value(&ok).is_ok());
}

#[test]
fn fault_keeps_an_explicit_fault_code() {
    let error = OperationError::new("xmlrpc_disabled", Category::Unavailable, "Disabled.")
        .with_details(Value::map([("fault_code", Value::Int(405))]));
    assert_eq!(
        Fault::from_error(&error, &Always500),
        Fault {
            code: 405,
            message: "Disabled.".into()
        }
    );
    let plain = OperationError::new("boom", Category::Internal, "Boom.");
    assert_eq!(Fault::from_error(&plain, &Always500).code, 500);
}
