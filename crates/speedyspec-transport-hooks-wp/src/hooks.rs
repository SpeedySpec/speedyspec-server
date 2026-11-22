//! The `TransportHooks` adapter over `HookApi`.

use std::rc::Rc;

use speedyspec_operation_contracts::{Category, OperationError, Value};
use speedyspec_transport_contracts::TransportHooks;
use speedyspec_wp_hooks::{HookApi, HookError, HookName, HookValue};

use crate::{from_hook, to_hook};

/// Fires transport hooks through WordPress's Hook API.
pub struct WpTransportHooks {
    api: Rc<HookApi>,
}

impl WpTransportHooks {
    pub fn new(api: Rc<HookApi>) -> Self {
        WpTransportHooks { api }
    }
}

fn hook_name(hook: &str) -> HookName {
    HookName::new(HookValue::Str(hook.to_owned()))
}

fn failed(error: HookError) -> OperationError {
    OperationError::new("hook_failed", Category::Internal, error.to_string())
}

impl TransportHooks for WpTransportHooks {
    fn action(&self, hook: &str, args: Vec<Value>) -> Result<(), OperationError> {
        self.api
            .do_action(&hook_name(hook), args.iter().map(to_hook).collect())
            .map_err(failed)
    }

    fn filter(&self, hook: &str, value: Value, args: Vec<Value>) -> Result<Value, OperationError> {
        let result = self
            .api
            .apply_filters(
                &hook_name(hook),
                to_hook(&value),
                args.iter().map(to_hook).collect(),
            )
            .map_err(failed)?;
        from_hook(&result)
    }
}
