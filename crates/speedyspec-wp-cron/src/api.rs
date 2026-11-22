//! [`CronApi`]: one method per `cron.php` function.

use std::rc::Rc;

use speedyspec_wp_cron_contracts::{
    ArgsKeyer, Clock, CronError, CronStore, DoingCron, ErrorReporter, PhpObjects, Translator,
    WakeReporter,
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
    /// Where the next due time is reported after every write and run.
    pub wake: Rc<dyn WakeReporter>,
    pub doing_cron: Rc<dyn DoingCron>,
    pub errors: Rc<dyn ErrorReporter>,
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

    /// `do_action( $hook, ...$args )`.
    pub(crate) fn action(&self, hook: &str, args: Vec<HookValue>) -> Result<(), CronError> {
        self.p
            .hooks
            .do_action(&HookName::from(hook), args)
            .map_err(hook_error)
    }

    pub(crate) fn tr(&self, text: &str) -> String {
        self.p.translator.translate(text)
    }
}
