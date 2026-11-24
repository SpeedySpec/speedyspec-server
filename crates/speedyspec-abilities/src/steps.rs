//! An operation's ability steps, in Rust.

use std::rc::Rc;

use speedyspec_abilities_contracts::{AbilityHooks, AbilityOutcome, AbilityResult, AbilitySteps};
use speedyspec_operation_contracts::{Caller, Category, OperationSpec, SchemaKind, Value};
use speedyspec_operation_dispatch::Dispatcher;

use crate::errors::{reason, wp_error};

/// An operation's steps with WordPress's semantics, its filters fired through the hooks:
///
/// - **normalize:** the input as given (operation schemas have no `default`), then `wp_ability_normalize_input`;
/// - **validate:** the operation's input schema, a failure being `ability_invalid_input` with WordPress's
///   `Reason:` text, then `wp_ability_validate_input`;
/// - **permission:** the operation's permission check, a refusal being `false`, then
///   `wp_ability_permission_result`;
/// - **execute:** the operation's handler (an invalid output is `ability_invalid_output`), then
///   `wp_ability_execute_result`;
/// - **output:** already validated by the dispatcher, then `wp_ability_validate_output`.
///
/// No input (`null`) is an empty object for an operation whose input is an object.
pub struct OperationSteps {
    dispatcher: Rc<Dispatcher>,
    spec: Rc<OperationSpec>,
    caller: Caller,
    hooks: Rc<dyn AbilityHooks>,
    ability: u64,
}

impl OperationSteps {
    pub fn new(
        dispatcher: Rc<Dispatcher>,
        spec: Rc<OperationSpec>,
        caller: Caller,
        hooks: Rc<dyn AbilityHooks>,
        ability: u64,
    ) -> OperationSteps {
        OperationSteps {
            dispatcher,
            spec,
            caller,
            hooks,
            ability,
        }
    }

    fn name(&self) -> &str {
        self.spec.name.as_str()
    }

    /// The neutral input the operation takes.
    fn value(&self, input: &Option<Value>) -> Value {
        match (input, &self.spec.input.schema.kind) {
            (Some(value), _) => value.clone(),
            (None, SchemaKind::Object { .. }) => Value::map(Vec::<(&str, Value)>::new()),
            (None, _) => Value::Null,
        }
    }
}

impl AbilitySteps for OperationSteps {
    fn normalize_input(&self, input: Option<Value>) -> AbilityResult<Option<Value>> {
        self.hooks.normalize_input(self.name(), input, self.ability)
    }

    fn validate_input(&self, input: &Option<Value>) -> AbilityResult<()> {
        let valid = self
            .dispatcher
            .validate(self.name(), &self.value(input))
            .map_err(|error| {
                wp_error(
                    "ability_invalid_input",
                    format!(
                        "Ability \"{}\" has invalid input. Reason: {}",
                        self.name(),
                        reason(&error)
                    ),
                )
            });
        self.hooks.validate_input(self.name(), valid, input)
    }

    fn check_permissions(&self, input: &Option<Value>) -> AbilityResult<bool> {
        let result = match self
            .dispatcher
            .authorize(self.name(), &self.value(input), &self.caller)
        {
            Ok(()) => Ok(true),
            Err(error)
                if matches!(
                    error.category,
                    Category::PermissionDenied | Category::Unauthenticated
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        };
        self.hooks
            .permission_result(self.name(), result, input, self.ability)
    }

    fn execute(&self, input: &Option<Value>) -> AbilityOutcome {
        let result = self
            .dispatcher
            .call(self.name(), self.value(input), &self.caller)
            .map_err(|error| {
                if error.code == "invalid_output" {
                    wp_error(
                        "ability_invalid_output",
                        format!(
                            "Ability \"{}\" has invalid output. Reason: {}",
                            self.name(),
                            reason(&error)
                        ),
                    )
                } else {
                    error
                }
            });
        self.hooks
            .execute_result(self.name(), result, input, self.ability)
    }

    fn validate_output(&self, output: &Value) -> AbilityResult<()> {
        self.hooks.validate_output(self.name(), Ok(()), output)
    }
}
