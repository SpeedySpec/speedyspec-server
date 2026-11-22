//! Data Mappers between neutral values and PHP value objects.
//!
//! Construction and reading happen in PHP (`Internal\Mapper`), by constructor parameter name and public property. A
//! hydrated value object travels through Rust as the PHP value itself (`speedyspec_php_ext::Value`), boxed.

use std::any::Any;

use speedyspec_operation_contracts::{Category, ContractMapper, OperationError, Value};
use speedyspec_php_ext::Value as PhpValue;

use crate::convert::{from_php, to_php};

const HYDRATE: &str = "SpeedySpec\\Operation\\Internal\\Mapper::hydrate";
const EXTRACT: &str = "SpeedySpec\\Operation\\Internal\\Mapper::extract";

fn class_value(class: &Option<String>) -> PhpValue {
    class.clone().map_or(PhpValue::Null, PhpValue::Str)
}

fn type_name(class: &Option<String>) -> &str {
    class.as_deref().unwrap_or("array")
}

/// Builds the input value object `class` (a plain array when `None`).
pub struct PhpInputMapper {
    pub class: Option<String>,
}

impl ContractMapper for PhpInputMapper {
    fn type_name(&self) -> &str {
        type_name(&self.class)
    }

    fn hydrate(&self, value: &Value) -> Result<Box<dyn Any>, OperationError> {
        let object = speedyspec_php::call(
            &PhpValue::Str(HYDRATE.into()),
            &[class_value(&self.class), to_php(value)],
        )
        .map_err(|err| {
            eprintln!(
                "speedyspec_wordpress: building {} failed: {err:?}",
                self.type_name()
            );
            OperationError::new(
                "hydration_failed",
                Category::Internal,
                format!("the input could not be built as {}", self.type_name()),
            )
        })?;
        Ok(Box::new(object))
    }

    fn extract(&self, object: &dyn Any) -> Result<Value, OperationError> {
        extract(object, type_name(&self.class))
    }
}

/// Reads the output value object `class` (a plain array when `None`).
pub struct PhpOutputMapper {
    pub class: Option<String>,
}

impl ContractMapper for PhpOutputMapper {
    fn type_name(&self) -> &str {
        type_name(&self.class)
    }

    fn hydrate(&self, value: &Value) -> Result<Box<dyn Any>, OperationError> {
        PhpInputMapper {
            class: self.class.clone(),
        }
        .hydrate(value)
    }

    fn extract(&self, object: &dyn Any) -> Result<Value, OperationError> {
        extract(object, type_name(&self.class))
    }
}

fn extract(object: &dyn Any, type_name: &str) -> Result<Value, OperationError> {
    let Some(object) = object.downcast_ref::<PhpValue>() else {
        return Err(OperationError::new(
            "output_type_mismatch",
            Category::Internal,
            format!("the handler did not return a PHP {type_name}"),
        ));
    };
    let plain = speedyspec_php::call(&PhpValue::Str(EXTRACT.into()), std::slice::from_ref(object))
        .map_err(|err| {
            eprintln!("speedyspec_wordpress: reading {type_name} failed: {err:?}");
            OperationError::new(
                "extraction_failed",
                Category::Internal,
                format!("the output could not be read as {type_name}"),
            )
        })?;
    from_php(&plain)
}
