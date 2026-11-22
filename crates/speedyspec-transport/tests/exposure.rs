//! Bindings, the transport registry and exposure.

mod common;

use std::rc::Rc;

use common::*;
use speedyspec_operation_contracts::*;
use speedyspec_transport::*;
use speedyspec_transport_contracts::*;

fn setup(transports: &[(&str, bool)]) -> (TransportRegistry, Rc<MemoryBindingStore>) {
    let store = Rc::new(MemoryBindingStore::new());
    let reg = TransportRegistry::new(store.clone());
    for (n, d) in transports {
        reg.install(Rc::new(FakeTransport::new(n, *d))).unwrap();
    }
    (reg, store)
}

fn names(list: Vec<Rc<ExposedOperation>>) -> Vec<String> {
    list.iter()
        .map(|e| e.spec.name.as_str().to_owned())
        .collect()
}

#[test]
fn unrestricted_operations_are_exposed_on_every_transport_by_default() {
    let (reg, _) = setup(&[("rest", true), ("xmlrpc", true)]);
    let ops = registries(vec![
        op("a/one").build().unwrap(),
        op("a/two").transports(&["rest"]).build().unwrap(),
    ]);
    let ex = reg.expose(&ops).unwrap();
    assert_eq!(names(ex.exposed("rest")), ["a/one", "a/two"]);
    assert_eq!(ex.exposed("xmlrpc").len(), 1);
    assert_eq!(
        ex.find("rest", "a/one").unwrap().source,
        BindingSource::Default
    );
    assert!(ex.find("xmlrpc", "a/two").is_none());
}

#[test]
fn a_transport_installed_later_exposes_unrestricted_operations() {
    let (reg, _) = setup(&[("rest", true)]);
    let ops = registries(vec![op("a/one").build().unwrap()]);
    assert!(reg.expose(&ops).unwrap().find("graphql", "a/one").is_none());
    reg.install(Rc::new(FakeTransport::new("graphql", true)))
        .unwrap();
    assert!(reg.expose(&ops).unwrap().find("graphql", "a/one").is_some());
}

#[test]
fn explicit_bindings_override_defaults_and_are_checked() {
    let (reg, store) = setup(&[("rest", false)]);
    let ops = registries(vec![
        op("a/one").build().unwrap(),
        op("a/two").build().unwrap(),
    ]);
    store.bind(
        "a/one",
        "rest",
        Value::map([("method", Value::from("one"))]),
    );
    let ex = reg.expose(&ops).unwrap();
    let one = ex.find("rest", "a/one").unwrap();
    assert_eq!(
        (one.source, one.binding.get("method")),
        (BindingSource::Explicit, Some(&Value::from("one")))
    );
    assert!(ex.find("rest", "a/two").is_none()); // no default mapping
    store.bind("a/two", "rest", Value::map::<&str>([]));
    let ex = reg.expose(&ops).unwrap();
    assert!(ex.find("rest", "a/two").is_none()); // the bad binding is skipped…
    assert!(ex.find("rest", "a/one").is_some()); // …and everything else is still served
    assert!(matches!(
        ex.problems(),
        [ExposureError::InvalidBinding { why, .. }] if why == "method is required"
    ));
}

#[test]
fn bindings_for_unknown_operations_or_outside_the_restriction_are_errors() {
    let (reg, store) = setup(&[("rest", true), ("xmlrpc", true)]);
    let ops = registries(vec![op("a/one").transports(&["rest"]).build().unwrap()]);
    store.bind("a/nope", "rest", Value::Null);
    store.bind(
        "a/one",
        "xmlrpc",
        Value::map([("method", Value::from("x"))]),
    );
    let ex = reg.expose(&ops).unwrap();
    let errs = ex.problems();
    assert!(ex.find("rest", "a/one").is_some()); // one plugin's bad binding does not take the site down
    assert!(errs.contains(&ExposureError::UnknownOperation {
        operation: "a/nope".into(),
        transport: "rest".into()
    }));
    assert!(errs.contains(&ExposureError::OutsideRestriction {
        operation: "a/one".into(),
        transport: "xmlrpc".into()
    }));
}

#[test]
fn bindings_for_transports_not_installed_are_ignored() {
    let (reg, store) = setup(&[("rest", true)]);
    let ops = registries(vec![op("a/one").build().unwrap()]);
    store.bind(
        "a/one",
        "graphql",
        Value::map([("field", Value::from("one"))]),
    );
    assert!(reg.expose(&ops).unwrap().problems().is_empty());
}

#[test]
fn open_registries_and_duplicate_transports_are_refused() {
    let (reg, _) = setup(&[("rest", true)]);
    assert_eq!(
        reg.expose(&speedyspec_operation::Registries::new())
            .unwrap_err(),
        ExposureError::RegistryOpen
    );
    assert_eq!(
        reg.install(Rc::new(FakeTransport::new("rest", true))),
        Err(ExposureError::DuplicateTransport {
            transport: "rest".into()
        })
    );
}
