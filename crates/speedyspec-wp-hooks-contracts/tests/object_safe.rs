//! The Hook API port is object safe: a seam holds it as `Rc<dyn HookApi>`.

use std::rc::Rc;

use speedyspec_wp_hooks_contracts::{
    Callback, CallbackId, Count, HasFilterResult, HookApi, HookError, HookName, HookValue,
};

/// Answers `"fixed"` to every filter, and nothing else.
struct Fixed;

impl HookApi for Fixed {
    fn skip_if_empty(&self, _hook: &HookName, _count: Count) -> Result<bool, HookError> {
        Ok(false)
    }

    fn add_filter(
        &self,
        _hook: &HookName,
        _callback: Callback,
        _priority: &HookValue,
        _accepted_args: i64,
    ) -> Result<bool, HookError> {
        Ok(false)
    }

    fn has_filter(
        &self,
        _hook: &HookName,
        _callback: Option<&CallbackId>,
        _priority: Option<i64>,
    ) -> Result<HasFilterResult, HookError> {
        Ok(HasFilterResult::No)
    }

    fn remove_filter(
        &self,
        _hook: &HookName,
        _callback: CallbackId,
        _priority: &HookValue,
    ) -> Result<bool, HookError> {
        Ok(false)
    }

    fn apply_filters(
        &self,
        _hook: &HookName,
        _value: HookValue,
        _args: Vec<HookValue>,
    ) -> Result<HookValue, HookError> {
        Ok(HookValue::Str("fixed".into()))
    }

    fn do_action(&self, _hook: &HookName, _args: Vec<HookValue>) -> Result<(), HookError> {
        Ok(())
    }

    fn do_action_ref_array(
        &self,
        _hook: &HookName,
        _args: Vec<HookValue>,
    ) -> Result<(), HookError> {
        Ok(())
    }

    fn do_action_deprecated(
        &self,
        _hook: &HookName,
        _args: Vec<HookValue>,
        _version: &HookValue,
        _replacement: &HookValue,
        _message: &HookValue,
    ) -> Result<(), HookError> {
        Ok(())
    }
}

#[test]
fn hook_api_is_object_safe() {
    let hooks: Rc<dyn HookApi> = Rc::new(Fixed);
    assert_eq!(
        hooks
            .apply_filters(&HookName::from("x"), HookValue::Null, Vec::new())
            .unwrap(),
        HookValue::Str("fixed".into())
    );
}
