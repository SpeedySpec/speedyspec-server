//! Stage middleware: each transport's hooks between the permission check and the handler.

use std::any::Any;
use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_operation_contracts::OperationError;
use speedyspec_operation_dispatch::{Bypass, Call, OperationMiddleware};
use speedyspec_transport_contracts::{StageHooks, TransportHooks};

/// Runs the caller's transport's [`StageHooks`] before the handler. Install it with `Dispatcher::with_middleware`, so it
/// runs after the permission check, as WordPress runs `rest_request_before_callbacks` after `permission_callback`.
pub struct StageMiddleware {
    hooks: Rc<dyn TransportHooks>,
    stages: BTreeMap<String, Rc<dyn StageHooks>>,
}

impl StageMiddleware {
    pub fn new(hooks: Rc<dyn TransportHooks>) -> Self {
        StageMiddleware {
            hooks,
            stages: BTreeMap::new(),
        }
    }

    /// The stage hooks of `transport`.
    pub fn stage(mut self, transport: &str, stage: Rc<dyn StageHooks>) -> Self {
        self.stages.insert(transport.to_owned(), stage);
        self
    }
}

impl OperationMiddleware for StageMiddleware {
    fn around(
        &self,
        call: &Call<'_>,
        next: &dyn Fn() -> Result<Box<dyn Any>, OperationError>,
    ) -> Result<Box<dyn Any>, OperationError> {
        let Some(stage) = self.stages.get(&call.caller.transport) else {
            return next();
        };
        match stage.before_handler(call.spec, call.caller, call.raw, self.hooks.as_ref())? {
            Some(answer) => Ok(Box::new(Bypass(answer))),
            None => next(),
        }
    }
}
