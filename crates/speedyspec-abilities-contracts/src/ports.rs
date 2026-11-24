//! The Abilities transport's ports.

use speedyspec_operation_contracts::{OperationError, OperationSpec, Value};

use crate::AbilityBinding;

/// What executing an ability gives: its output, or the error (a `WP_Error`, as an operation error).
pub type AbilityOutcome = Result<Value, OperationError>;
/// A step's result.
pub type AbilityResult<T> = Result<T, OperationError>;

/// Which metadata an operation is published with when it has no explicit binding (Strategy).
pub trait AbilityMapping {
    /// The operation's ability metadata, or `None` when it cannot be an ability.
    fn binding(&self, spec: &OperationSpec) -> Option<AbilityBinding>;
}

/// WordPress's ability hooks, one method per hook. `ability` is the handle of the `WP_Ability` being executed, opaque
/// to Rust and passed back to the adapter, which gives hooks the object itself.
pub trait AbilityHooks {
    /// `wp_ability_invoked`.
    fn invoked(
        &self,
        name: &str,
        input: &Option<Value>,
        ability: u64,
    ) -> Result<(), OperationError>;
    /// `wp_pre_execute_ability`: `Some` short-circuits `execute()` with that outcome (the filter returned something
    /// other than its sentinel).
    fn pre_execute(
        &self,
        name: &str,
        input: &Option<Value>,
        ability: u64,
    ) -> Result<Option<AbilityOutcome>, OperationError>;
    /// `wp_ability_normalize_input`.
    fn normalize_input(
        &self,
        name: &str,
        input: Option<Value>,
        ability: u64,
    ) -> AbilityResult<Option<Value>>;
    /// `wp_ability_validate_input`.
    fn validate_input(
        &self,
        name: &str,
        valid: AbilityResult<()>,
        input: &Option<Value>,
    ) -> AbilityResult<()>;
    /// `wp_ability_permission_result`.
    fn permission_result(
        &self,
        name: &str,
        result: AbilityResult<bool>,
        input: &Option<Value>,
        ability: u64,
    ) -> AbilityResult<bool>;
    /// `wp_before_execute_ability`.
    fn before_execute(
        &self,
        name: &str,
        input: &Option<Value>,
        ability: u64,
    ) -> Result<(), OperationError>;
    /// `wp_ability_execute_result`.
    fn execute_result(
        &self,
        name: &str,
        result: AbilityOutcome,
        input: &Option<Value>,
        ability: u64,
    ) -> AbilityOutcome;
    /// `wp_ability_validate_output`.
    fn validate_output(
        &self,
        name: &str,
        valid: AbilityResult<()>,
        output: &Value,
    ) -> AbilityResult<()>;
    /// `wp_after_execute_ability`.
    fn after_execute(
        &self,
        name: &str,
        input: &Option<Value>,
        output: &Value,
        ability: u64,
    ) -> Result<(), OperationError>;
    /// WordPress's `_doing_it_wrong()`.
    fn doing_it_wrong(&self, function: &str, message: &str, version: &str);
}

/// WordPress's behavior with no hooks added: every action does nothing and every filter returns its value.
#[derive(Default)]
pub struct NoAbilityHooks;

impl AbilityHooks for NoAbilityHooks {
    fn invoked(&self, _: &str, _: &Option<Value>, _: u64) -> Result<(), OperationError> {
        Ok(())
    }
    fn pre_execute(
        &self,
        _: &str,
        _: &Option<Value>,
        _: u64,
    ) -> Result<Option<AbilityOutcome>, OperationError> {
        Ok(None)
    }
    fn normalize_input(
        &self,
        _: &str,
        input: Option<Value>,
        _: u64,
    ) -> AbilityResult<Option<Value>> {
        Ok(input)
    }
    fn validate_input(
        &self,
        _: &str,
        valid: AbilityResult<()>,
        _: &Option<Value>,
    ) -> AbilityResult<()> {
        valid
    }
    fn permission_result(
        &self,
        _: &str,
        result: AbilityResult<bool>,
        _: &Option<Value>,
        _: u64,
    ) -> AbilityResult<bool> {
        result
    }
    fn before_execute(&self, _: &str, _: &Option<Value>, _: u64) -> Result<(), OperationError> {
        Ok(())
    }
    fn execute_result(
        &self,
        _: &str,
        result: AbilityOutcome,
        _: &Option<Value>,
        _: u64,
    ) -> AbilityOutcome {
        result
    }
    fn validate_output(&self, _: &str, valid: AbilityResult<()>, _: &Value) -> AbilityResult<()> {
        valid
    }
    fn after_execute(
        &self,
        _: &str,
        _: &Option<Value>,
        _: &Value,
        _: u64,
    ) -> Result<(), OperationError> {
        Ok(())
    }
    fn doing_it_wrong(&self, _: &str, _: &str, _: &str) {}
}

/// One ability's steps, mirroring `WP_Ability`'s methods with the filters they fire (Strategy): Rust's for an
/// operation, the PHP object's own for any other ability.
pub trait AbilitySteps {
    /// `normalize_input()`.
    fn normalize_input(&self, input: Option<Value>) -> AbilityResult<Option<Value>>;
    /// `validate_input()`.
    fn validate_input(&self, input: &Option<Value>) -> AbilityResult<()>;
    /// `check_permissions()`: `Ok(true)` allows; `Ok(false)` or an error refuses.
    fn check_permissions(&self, input: &Option<Value>) -> AbilityResult<bool>;
    /// `do_execute()`, its `wp_ability_execute_result` filter included.
    fn execute(&self, input: &Option<Value>) -> AbilityOutcome;
    /// `validate_output()`.
    fn validate_output(&self, output: &Value) -> AbilityResult<()>;
}
