//! The Abilities server (Template Method): `WP_Ability::execute()` in WordPress's order.

use std::rc::Rc;

use speedyspec_abilities_contracts::{AbilityHooks, AbilityOutcome, AbilitySteps};
use speedyspec_operation_contracts::Value;

use crate::errors::{hook_failed, invalid_permissions};

/// Runs abilities as WordPress 7.1.5's `WP_Ability::execute()` does: `wp_ability_invoked`, `wp_pre_execute_ability`
/// (any value but its sentinel short-circuits), normalize, validate, permission (anything but `true` is
/// `ability_invalid_permissions`, a `WP_Error` reported with `_doing_it_wrong`), `wp_before_execute_ability`, execute,
/// validate the output, `wp_after_execute_ability`. The steps are the ability's ([`AbilitySteps`]); their filters fire
/// inside them, as they fire inside WordPress's methods. A hook that fails fails the call closed.
pub struct AbilitiesServer {
    hooks: Rc<dyn AbilityHooks>,
}

impl AbilitiesServer {
    pub fn new(hooks: Rc<dyn AbilityHooks>) -> AbilitiesServer {
        AbilitiesServer { hooks }
    }

    /// The hooks this server fires.
    pub fn hooks(&self) -> &Rc<dyn AbilityHooks> {
        &self.hooks
    }

    /// Executes the ability `name` (its PHP object's handle is `ability`) with `input`.
    pub fn execute(
        &self,
        name: &str,
        input: Option<Value>,
        steps: &dyn AbilitySteps,
        ability: u64,
    ) -> AbilityOutcome {
        self.hooks
            .invoked(name, &input, ability)
            .map_err(|_| hook_failed())?;
        if let Some(outcome) = self
            .hooks
            .pre_execute(name, &input, ability)
            .map_err(|_| hook_failed())?
        {
            return outcome;
        }
        let input = steps.normalize_input(input)?;
        steps.validate_input(&input)?;
        match steps.check_permissions(&input) {
            Ok(true) => {}
            Ok(false) => return Err(invalid_permissions(name)),
            Err(error) => {
                // As WordPress: the reason is not told to a caller without permission, only reported.
                self.hooks
                    .doing_it_wrong("WP_Ability::execute", &error.message, "6.9.0");
                return Err(invalid_permissions(name));
            }
        }
        self.hooks
            .before_execute(name, &input, ability)
            .map_err(|_| hook_failed())?;
        let output = steps.execute(&input)?;
        steps.validate_output(&output)?;
        self.hooks
            .after_execute(name, &input, &output, ability)
            .map_err(|_| hook_failed())?;
        Ok(output)
    }
}
