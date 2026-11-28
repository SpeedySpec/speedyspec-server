//! `update_option` with a compare-and-swap write, for writers that read, change and write back an option that other
//! processes change too (cron's `cron` option). Optimistic Offline Lock: the version is the stored bytes as read.

use speedyspec_wp_hooks_contracts::HookValue;

use crate::api::{OptionsApi, isset, unset_key};
use crate::write::PlannedWrite;
use crate::{ConditionalWrite, OptionName, OptionsError, SerializedValue};

impl OptionsApi {
    /// `update_option( $option, $value, $autoload )`, whose write happens only where the option's stored bytes are
    /// still `expected` (`None`: the option was absent, so the write inserts and never overwrites).
    ///
    /// A caller whose read disagrees with this process's caches (it expected a row the caches say is missing, or the
    /// reverse) gets [`ConditionalWrite::Conflict`] without a write, and reloads.
    pub fn update_option_if_unchanged(
        &self,
        name: &str,
        expected: Option<&[u8]>,
        value: HookValue,
        autoload: HookValue,
    ) -> Result<ConditionalWrite, OptionsError> {
        let Some(planned) = self.plan_update(name, value, autoload)? else {
            return Ok(ConditionalWrite::Unchanged);
        };
        let rows = match (&planned, expected) {
            (
                PlannedWrite::Update {
                    name,
                    stored,
                    autoload,
                    ..
                },
                Some(bytes),
            ) => self.d.repository.update_if(
                name,
                &SerializedValue::new(bytes.to_vec()),
                SerializedValue::for_update(stored)?.as_ref(),
                autoload.as_ref(),
            )?,
            (PlannedWrite::Add { option, .. }, None) => self.d.repository.add_if_absent(option)?,
            _ => 0,
        };
        if rows == 0 {
            return Ok(ConditionalWrite::Conflict);
        }
        Ok(if self.finish(&planned, rows)? {
            ConditionalWrite::Written
        } else {
            ConditionalWrite::Unchanged
        })
    }

    /// Drops what this process's object cache holds for `name`: the single option, its slot in `alloptions` and its
    /// `notoptions` entry. The next `get_option` reads the repository.
    pub fn forget_cached(&self, name: &str) -> Result<(), OptionsError> {
        let Some(option) = OptionName::new(name) else {
            return Ok(());
        };
        let group = Self::group();
        let cache = self.cache();
        cache.delete(&HookValue::Str(option.as_str().to_string()), &group)?;
        for key in ["alloptions", "notoptions"] {
            let key = HookValue::Str(key.into());
            if let Some(held) = cache.get(&key, &group, false)? {
                let mut held = held.read().into_owned();
                if matches!(held, HookValue::Array(_)) && isset(&held, option.as_str()).is_some() {
                    unset_key(&mut held, option.as_str());
                    cache.set(&key, held, &group, 0)?;
                }
            }
        }
        Ok(())
    }

    /// The bytes the repository holds for `name`, with no hook and no cache; `None` when there is no row.
    pub fn stored_bytes(&self, name: &str) -> Result<Option<Vec<u8>>, OptionsError> {
        let Some(option) = OptionName::new(name) else {
            return Ok(None);
        };
        Ok(self
            .d
            .repository
            .value(&option, true)?
            .map(|v| v.as_bytes().to_vec()))
    }

    /// The bytes `update_option` would store for `value` (`maybe_serialize`).
    pub fn serialized_form(&self, value: &HookValue) -> Result<Vec<u8>, OptionsError> {
        let stored = self.d.codec.maybe_serialize(value)?;
        Ok(SerializedValue::for_update(&stored)?
            .map(|v| v.as_bytes().to_vec())
            .unwrap_or_default())
    }
}
