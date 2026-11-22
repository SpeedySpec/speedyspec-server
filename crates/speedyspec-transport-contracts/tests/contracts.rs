//! Transport names, port object safety and exposure errors.

use std::rc::Rc;

use speedyspec_operation_contracts::*;
use speedyspec_transport_contracts::*;

#[test]
fn transport_names_are_lowercase_and_internal_is_reserved() {
    assert_eq!(TransportName::parse("rest").unwrap().as_str(), "rest");
    assert!(TransportName::parse("json-rpc2").is_ok());
    for bad in ["", "REST", "x_rpc", "a/b", "internal"] {
        assert!(TransportName::parse(bad).is_err(), "{bad}");
    }
}

#[test]
#[allow(clippy::type_complexity)]
fn ports_are_object_safe() {
    fn takes(
        _: Option<&dyn Transport>,
        _: Option<&dyn BindingStore>,
        _: Option<&dyn ExposureLookup>,
        _: Option<&dyn Authenticator<String>>,
        _: Option<&dyn Codec<String, String>>,
        _: Option<&dyn TransportHooks>,
        _: Option<&dyn StageHooks>,
    ) {
    }
    takes(None, None, None, None, None, None, None);
    let _ = Rc::new(DecodedCall {
        operation: "a/b".into(),
        input: Value::Null,
        requested_fields: None,
    });
}

#[test]
fn exposure_errors_name_the_operation_and_transport() {
    let e = ExposureError::OutsideRestriction {
        operation: "a/b".into(),
        transport: "xmlrpc".into(),
    };
    assert!(e.to_string().contains("a/b") && e.to_string().contains("xmlrpc"));
}
