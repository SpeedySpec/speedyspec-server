//! [`CronApi`]: one method per `cron.php` function.

use std::rc::Rc;

use speedyspec_wp_cron_contracts::{
    ArgsKeyer, Clock, CronError, CronStore, PhpObjects, Translator,
};
use speedyspec_wp_hooks::{HookApi, HookError, HookName, HookValue};

/// What [`CronApi`] is built from: the Hook API and the ports.
pub struct CronParts {
    pub hooks: Rc<HookApi>,
    pub store: Rc<dyn CronStore>,
    pub keyer: Rc<dyn ArgsKeyer>,
    pub clock: Rc<dyn Clock>,
    pub translator: Rc<dyn Translator>,
    pub objects: Rc<dyn PhpObjects>,
}

/// WordPress's cron API. Each method takes PHP's arguments as passed and answers as WordPress does.
pub struct CronApi {
    pub(crate) p: CronParts,
}

pub(crate) fn hook_error(e: HookError) -> CronError {
    CronError {
        message: e.message,
        payload: e.payload,
    }
}

impl CronApi {
    pub fn new(parts: CronParts) -> CronApi {
        CronApi { p: parts }
    }

    /// `apply_filters( $hook, $value, ...$args )`.
    pub(crate) fn filter(
        &self,
        hook: &str,
        value: HookValue,
        args: Vec<HookValue>,
    ) -> Result<HookValue, CronError> {
        self.p
            .hooks
            .apply_filters(&HookName::from(hook), value, args)
            .map_err(hook_error)
    }

    pub(crate) fn tr(&self, text: &str) -> String {
        self.p.translator.translate(text)
    }
}
