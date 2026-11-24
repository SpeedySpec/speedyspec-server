//! WordPress's ability hooks, through `do_action` and `apply_filters`.

use speedyspec_abilities::errors::hook_failed;
use speedyspec_abilities_contracts::{AbilityHooks, AbilityOutcome, AbilityResult};
use speedyspec_operation_contracts::{OperationError, Value};

use speedyspec_php_ext::Value as PhpValue;

use crate::errors::{outcome, to_wp_error};
use crate::handles;

fn wordpress_loaded() -> bool {
    matches!(
        speedyspec_php::call(
            &PhpValue::Str("function_exists".into()),
            &[PhpValue::Str("apply_filters".into())],
        ),
        Ok(PhpValue::Bool(true))
    )
}

/// Calls the adapter, catching what a hook throws: the call then fails closed (logged, never told to the client).
fn guarded(method: &str, args: &[PhpValue]) -> Result<PhpValue, OperationError> {
    speedyspec_php::call_catching(
        &PhpValue::Str(format!(
            "SpeedySpec\\Abilities\\Internal\\Adapter::{method}"
        )),
        args,
    )
    .map_err(|failure| {
        eprintln!(
            "speedyspec_wordpress: an ability hook failed in {method}: {}",
            failure.error
        );
        hook_failed()
    })
}

fn php_input(input: &Option<Value>) -> PhpValue {
    input.as_ref().map_or(PhpValue::Null, crate::values::php)
}

fn neutral(value: &PhpValue) -> Value {
    crate::values::neutral(value)
}

fn valid_php(valid: &AbilityResult<()>) -> PhpValue {
    match valid {
        Ok(()) => PhpValue::Bool(true),
        Err(error) => to_wp_error(error),
    }
}

/// WordPress's ability hooks, with WordPress's names, arguments and coercions; the ability argument is the
/// `WP_Ability` itself. Where WordPress's hooks are not loaded, each gives WordPress's no-hook answer. A hook that
/// throws fails closed with `ability_hook_failed`.
#[derive(Default)]
pub struct PhpAbilityHooks;

impl PhpAbilityHooks {
    pub fn new() -> PhpAbilityHooks {
        PhpAbilityHooks
    }
}

fn name_php(name: &str) -> PhpValue {
    PhpValue::Str(name.to_owned())
}

impl AbilityHooks for PhpAbilityHooks {
    fn invoked(
        &self,
        name: &str,
        input: &Option<Value>,
        ability: u64,
    ) -> Result<(), OperationError> {
        if !wordpress_loaded() {
            return Ok(());
        }
        guarded(
            "invoked",
            &[name_php(name), php_input(input), handles::get(ability)],
        )
        .map(drop)
    }

    fn pre_execute(
        &self,
        name: &str,
        input: &Option<Value>,
        ability: u64,
    ) -> Result<Option<AbilityOutcome>, OperationError> {
        if !wordpress_loaded() {
            return Ok(None);
        }
        let result = guarded(
            "pre_execute",
            &[name_php(name), php_input(input), handles::get(ability)],
        )?;
        if let PhpValue::Array(items) = &result
            && matches!(items.first(), Some((_, PhpValue::Str(kind))) if kind == "run")
        {
            return Ok(None);
        }
        Ok(Some(outcome(&result).map(|v| neutral(&v))))
    }

    fn normalize_input(
        &self,
        name: &str,
        input: Option<Value>,
        ability: u64,
    ) -> AbilityResult<Option<Value>> {
        if !wordpress_loaded() {
            return Ok(input);
        }
        let result = guarded(
            "normalize_input",
            &[name_php(name), php_input(&input), handles::get(ability)],
        )?;
        outcome(&result).map(|v| match v {
            PhpValue::Null => None,
            v => Some(neutral(&v)),
        })
    }

    fn validate_input(
        &self,
        name: &str,
        valid: AbilityResult<()>,
        input: &Option<Value>,
    ) -> AbilityResult<()> {
        if !wordpress_loaded() {
            return valid;
        }
        let result = guarded(
            "validate_input",
            &[name_php(name), valid_php(&valid), php_input(input)],
        )?;
        outcome(&result).map(drop)
    }

    fn permission_result(
        &self,
        name: &str,
        result: AbilityResult<bool>,
        input: &Option<Value>,
        ability: u64,
    ) -> AbilityResult<bool> {
        if !wordpress_loaded() {
            return result;
        }
        let permission = match &result {
            Ok(allowed) => PhpValue::Bool(*allowed),
            Err(error) => to_wp_error(error),
        };
        let filtered = guarded(
            "permission_result",
            &[
                name_php(name),
                permission,
                php_input(input),
                handles::get(ability),
            ],
        )?;
        outcome(&filtered).map(|v| matches!(v, PhpValue::Bool(true)))
    }

    fn before_execute(
        &self,
        name: &str,
        input: &Option<Value>,
        ability: u64,
    ) -> Result<(), OperationError> {
        if !wordpress_loaded() {
            return Ok(());
        }
        guarded(
            "before_execute",
            &[name_php(name), php_input(input), handles::get(ability)],
        )
        .map(drop)
    }

    fn execute_result(
        &self,
        name: &str,
        result: AbilityOutcome,
        input: &Option<Value>,
        ability: u64,
    ) -> AbilityOutcome {
        if !wordpress_loaded() {
            return result;
        }
        let php = match &result {
            Ok(value) => crate::values::php(value),
            Err(error) => to_wp_error(error),
        };
        let filtered = guarded(
            "execute_result",
            &[name_php(name), php, php_input(input), handles::get(ability)],
        )?;
        outcome(&filtered).map(|v| neutral(&v))
    }

    fn validate_output(
        &self,
        name: &str,
        valid: AbilityResult<()>,
        output: &Value,
    ) -> AbilityResult<()> {
        if !wordpress_loaded() {
            return valid;
        }
        let result = guarded(
            "validate_output",
            &[
                name_php(name),
                valid_php(&valid),
                crate::values::php(output),
            ],
        )?;
        outcome(&result).map(drop)
    }

    fn after_execute(
        &self,
        name: &str,
        input: &Option<Value>,
        output: &Value,
        ability: u64,
    ) -> Result<(), OperationError> {
        if !wordpress_loaded() {
            return Ok(());
        }
        guarded(
            "after_execute",
            &[
                name_php(name),
                php_input(input),
                crate::values::php(output),
                handles::get(ability),
            ],
        )
        .map(drop)
    }

    fn doing_it_wrong(&self, function: &str, message: &str, version: &str) {
        let _ = guarded(
            "doing_it_wrong",
            &[name_php(function), name_php(message), name_php(version)],
        );
    }
}
