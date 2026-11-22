//! Fixtures shared by the transport tests.
#![allow(dead_code)]

use std::rc::Rc;

use speedyspec_operation::{InputMapper, Operation, OperationBuilder, OutputMapper, Registries, S};
use speedyspec_operation_contracts::*;
use speedyspec_transport_contracts::{Transport, TransportName};

/// Binds operations by a `method` string; defaults to the operation name when `defaults`.
pub struct FakeTransport {
    name: TransportName,
    defaults: bool,
}

impl FakeTransport {
    pub fn new(name: &str, defaults: bool) -> Self {
        FakeTransport {
            name: TransportName::parse(name).unwrap(),
            defaults,
        }
    }
}

impl Transport for FakeTransport {
    fn name(&self) -> &TransportName {
        &self.name
    }

    fn default_binding(&self, spec: &OperationSpec) -> Option<Value> {
        self.defaults
            .then(|| Value::map([("method", Value::from(spec.name.as_str()))]))
    }

    fn check_binding(&self, _: &OperationSpec, binding: &Value) -> Result<(), String> {
        match binding.get("method") {
            Some(Value::String(_)) => Ok(()),
            _ => Err("method is required".into()),
        }
    }
}

/// A public query with raw contracts.
pub fn op(name: &str) -> OperationBuilder {
    Operation::query(name)
        .input_with(S::any(), Rc::new(InputMapper::<RawInput>::new()))
        .output_with(S::any(), Rc::new(OutputMapper::<RawOutput>::new()))
        .public()
}

/// Closed registries holding `ops`.
pub fn registries(ops: Vec<OperationSpec>) -> Registries {
    let r = Registries::new();
    for spec in ops {
        r.register_operation(spec).unwrap();
    }
    r.close().unwrap();
    r
}
