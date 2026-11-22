//! [`OptionCronStore`]: the `cron` option through the options port, every write a compare-and-swap.
//!
//! The version a save compares is the bytes `update_option` would store for the value read
//! (`OptionsApi::serialized_form`), so the conditional write matches only if no one wrote since. A value a filter
//! rewrote on read (`pre_option_cron`, `option_cron`) is not the stored one; [`OptionCronStore::reload`] says so, and
//! the core then writes unconditionally, as WordPress does.

use std::any::Any;
use std::rc::Rc;

use speedyspec_wp_cron_contracts::{CronError, CronStore, SaveOutcome, Version};
use speedyspec_wp_hooks::HookValue;
use speedyspec_wp_options::{ConditionalWrite, OptionsApi, OptionsError};

const CRON: &str = "cron";

/// The `cron` option over the options port (Adapter).
pub struct OptionCronStore {
    options: Rc<OptionsApi>,
}

fn error(e: OptionsError) -> CronError {
    match e {
        OptionsError::Hook(hook) => CronError {
            message: hook.message,
            payload: hook.payload,
        },
        OptionsError::Php(value) => CronError {
            message: "a PHP callback threw an exception".into(),
            payload: Some(Rc::new(value) as Rc<dyn Any>),
        },
        other => CronError::new(other.to_string()),
    }
}

impl OptionCronStore {
    pub fn new(options: Rc<OptionsApi>) -> OptionCronStore {
        OptionCronStore { options }
    }
}

impl CronStore for OptionCronStore {
    fn load(&self) -> Result<(HookValue, Version), CronError> {
        let value = self.options.get_option(CRON, None).map_err(error)?;
        let version = match &*value.read() {
            // get_option()'s default: the option is absent, unless a stored row holds false itself.
            HookValue::Bool(false) => match self.options.stored_bytes(CRON).map_err(error)? {
                None => Version(None),
                Some(bytes) => Version(Some(bytes)),
            },
            _ => Version(Some(self.options.serialized_form(&value).map_err(error)?)),
        };
        Ok((value, version))
    }

    /// The value is reported as rewritten when it is `false` while a row exists, or when the stored bytes read before
    /// and after it agree with each other and not with it. When another process writes between the reads, they disagree; the value is then reported as
    /// stored, so the caller keeps to compare-and-swap (which can only conflict) rather than writing unconditionally
    /// over the other process's write.
    fn reload(&self) -> Result<(HookValue, Version, bool), CronError> {
        let before = self.options.stored_bytes(CRON).map_err(error)?;
        self.options.forget_cached(CRON).map_err(error)?;
        let (value, version) = self.load()?;
        let after = self.options.stored_bytes(CRON).map_err(error)?;
        // `false` while a row exists (a filter hid the option, or the row holds `b:0;`) makes `update_option()` add
        // rather than update, which a compare-and-swap of the row can never match; WordPress's own write is used.
        let hidden = matches!(&*value.read(), HookValue::Bool(false)) && after.is_some();
        let rewritten = hidden || (before == after && after != version.0);
        Ok((value, version, !rewritten))
    }

    fn save_if(&self, expected: &Version, value: HookValue) -> Result<SaveOutcome, CronError> {
        Ok(
            match self
                .options
                .update_option_if_unchanged(
                    CRON,
                    expected.0.as_deref(),
                    value,
                    HookValue::Bool(true),
                )
                .map_err(error)?
            {
                ConditionalWrite::Written => SaveOutcome::Written,
                ConditionalWrite::Unchanged => SaveOutcome::Unchanged,
                ConditionalWrite::Conflict => SaveOutcome::Conflict,
            },
        )
    }

    fn save(&self, value: HookValue) -> Result<bool, CronError> {
        self.options
            .update_option(CRON, value, HookValue::Bool(true))
            .map_err(error)
    }
}
