//! The contract round-trip test helper.

use speedyspec_operation::testing::assert_contract_round_trip;
use speedyspec_operation::{Registries, S};
use speedyspec_operation_contracts::*;

#[derive(Debug, PartialEq)]
struct Title(String);

impl Hydrate for Title {
    fn hydrate(v: &Value) -> Result<Self, OperationError> {
        match v.get("title") {
            Some(Value::String(s)) => Ok(Title(s.clone())),
            _ => Err(OperationError::new(
                "bad",
                Category::InvalidArgument,
                "title",
            )),
        }
    }
}

impl Extract for Title {
    fn extract(&self) -> Value {
        Value::map([("title", Value::from(self.0.as_str()))])
    }
}

#[test]
fn matching_contract_passes() {
    assert_contract_round_trip(
        &S::object([("title", S::string().required())]).build(),
        &Registries::new(),
        &Title("a".into()),
    );
}

#[test]
#[should_panic(expected = "does not match its schema")]
fn schema_drift_panics() {
    assert_contract_round_trip(
        &S::object([("name", S::string().required())]).build(),
        &Registries::new(),
        &Title("a".into()),
    );
}
