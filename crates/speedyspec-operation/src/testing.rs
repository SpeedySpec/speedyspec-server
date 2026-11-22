//! Test helpers for contracts.

use std::fmt::Debug;

use speedyspec_operation_contracts::{Extract, Hydrate, Schema, TypeCatalog};

use crate::validate::validate;

/// Panics unless `sample` extracts to a value its `schema` accepts and hydrates back to itself. The Rust form of the
/// registration check that a schema and its value object agree; run it in each contract's tests.
pub fn assert_contract_round_trip<T: Hydrate + Extract + PartialEq + Debug>(
    schema: &Schema,
    types: &dyn TypeCatalog,
    sample: &T,
) {
    let name = std::any::type_name::<T>();
    let value = sample.extract();
    if let Err(violations) = validate(schema, &value, types) {
        panic!("contract {name} does not match its schema: {violations:?}");
    }
    match T::hydrate(&value) {
        Ok(back) if &back == sample => {}
        _ => panic!("contract {name} does not round-trip"),
    }
}
