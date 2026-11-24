//! Executing one ability: the right steps for it, through the Abilities server.

use speedyspec_abilities_contracts::AbilityOutcome;
use speedyspec_operation_contracts::Value;

use crate::AbilitiesService;
use crate::steps::PhpAbilitySteps;

/// Executes the ability `name` (its object `object`, handle `handle`) with `input`.
pub(crate) fn run(
    service: &AbilitiesService,
    name: &str,
    object: i64,
    handle: u64,
    input: Option<Value>,
) -> AbilityOutcome {
    let (dispatcher, operation) =
        crate::legacy::operation_for(service.container(), name, object, handle)?;
    let steps = PhpAbilitySteps::new(handle, dispatcher, operation, service.caller());
    service.server().execute(name, input, &steps, handle)
}
