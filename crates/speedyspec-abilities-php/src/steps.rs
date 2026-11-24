//! A `WP_Ability`'s own steps, for the Abilities server: the LegacyHandlerAdapter's half that runs in PHP.

use std::rc::Rc;

use speedyspec_abilities_contracts::{AbilityOutcome, AbilityResult, AbilitySteps};
use speedyspec_operation_contracts::{Caller, OperationError, Value};
use speedyspec_operation_dispatch::Dispatcher;
use speedyspec_operation_php::convert::{from_php, to_php};
use speedyspec_php_ext::{Key, Value as PhpValue};

use crate::errors::outcome;
use crate::handles;

/// Runs the step `step` of the ability `handle` stands for, with `input`: its PHP result, or its `WP_Error`.
pub(crate) fn step(
    handle: u64,
    step: &str,
    input: &Option<Value>,
) -> Result<PhpValue, OperationError> {
    let args = PhpValue::Array(vec![(
        Key::Int(0),
        input.as_ref().map_or(PhpValue::Null, to_php),
    )]);
    let result = speedyspec_php::call_catching(
        &PhpValue::Str("SpeedySpec\\Abilities\\Internal\\Adapter::step".into()),
        &[handles::get(handle), PhpValue::Str(step.into()), args],
    )
    .map_err(|failure| {
        eprintln!(
            "speedyspec_wordpress: the ability step {step} failed: {}",
            failure.error
        );
        speedyspec_abilities::errors::hook_failed()
    })?;
    outcome(&result)
}

/// A `WP_Ability`'s steps: each is the object's own method (so a subclass's override runs), firing its own filters
/// as WordPress's methods do. Executing goes through the ability's operation, so the core's middleware wraps it.
pub struct PhpAbilitySteps {
    handle: u64,
    dispatcher: Rc<Dispatcher>,
    operation: String,
    caller: Caller,
}

impl PhpAbilitySteps {
    pub fn new(
        handle: u64,
        dispatcher: Rc<Dispatcher>,
        operation: String,
        caller: Caller,
    ) -> PhpAbilitySteps {
        PhpAbilitySteps {
            handle,
            dispatcher,
            operation,
            caller,
        }
    }
}

impl AbilitySteps for PhpAbilitySteps {
    fn normalize_input(&self, input: Option<Value>) -> AbilityResult<Option<Value>> {
        step(self.handle, "normalize_input", &input).map(|v| match v {
            PhpValue::Null => None,
            v => Some(from_php(&v).unwrap_or(Value::Null)),
        })
    }

    fn validate_input(&self, input: &Option<Value>) -> AbilityResult<()> {
        step(self.handle, "validate_input", input).map(drop)
    }

    fn check_permissions(&self, input: &Option<Value>) -> AbilityResult<bool> {
        step(self.handle, "check_permissions", input).map(|v| matches!(v, PhpValue::Bool(true)))
    }

    fn execute(&self, input: &Option<Value>) -> AbilityOutcome {
        self.dispatcher.call(
            &self.operation,
            crate::legacy::input_value(input),
            &self.caller,
        )
    }

    fn validate_output(&self, output: &Value) -> AbilityResult<()> {
        step(self.handle, "validate_output", &Some(output.clone())).map(drop)
    }
}
