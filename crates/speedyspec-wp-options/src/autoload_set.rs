//! `wp_set_option_autoload_values`, `wp_set_options_autoload` and `wp_set_option_autoload`.

use speedyspec_wp_hooks_contracts::HookValue;

use crate::api::{OptionsApi, key_of, unset_key};
use crate::{Autoload, OptionsError};

impl OptionsApi {
    /// `wp_set_option_autoload_values( $options )`: one `SELECT`, at most two `UPDATE`s; per name, whether its
    /// autoload changed.
    pub fn set_option_autoload_values(
        &self,
        options: Vec<(String, HookValue)>,
    ) -> Result<Vec<(String, bool)>, OptionsError> {
        if options.is_empty() {
            return Ok(Vec::new());
        }
        let mut on: Vec<String> = Vec::new();
        let mut off: Vec<String> = Vec::new();
        let mut results: Vec<(String, bool)> = Vec::new();
        for (name, autoload) in options {
            self.d.protected.protect(&name)?;
            let is_off = match autoload.read().as_ref() {
                HookValue::Str(s) => s == "off" || s == "no",
                HookValue::Bool(false) => true,
                _ => false,
            };
            // A PHP array has one entry per key: a repeated name keeps its first place and its last group.
            on.retain(|n| key_of(n) != key_of(&name));
            off.retain(|n| key_of(n) != key_of(&name));
            if is_off {
                off.push(name.clone())
            } else {
                on.push(name.clone())
            }
            if !results.iter().any(|(n, _)| key_of(n) == key_of(&name)) {
                results.push((name, false));
            }
        }
        let groups: Vec<(Autoload, Vec<String>)> =
            [(Autoload::On, on.clone()), (Autoload::Off, off.clone())]
                .into_iter()
                .filter(|(_, names)| !names.is_empty())
                .collect();
        let to_update = self.d.repository.needing_autoload_change(&groups)?;
        if to_update.is_empty() {
            return Ok(results);
        }
        for (autoload, names) in [(Autoload::On, &mut on), (Autoload::Off, &mut off)] {
            if names.is_empty() {
                continue;
            }
            names.retain(|n| to_update.contains(n));
            if names.is_empty() {
                continue;
            }
            if self.d.repository.set_autoload(names, &autoload)? == 0 {
                names.clear();
                continue;
            }
            for (name, changed) in results.iter_mut() {
                if names.contains(name) {
                    *changed = true;
                }
            }
        }
        let group = Self::group();
        let alloptions_key = HookValue::Str("alloptions".into());
        if !on.is_empty() {
            let keys: Vec<HookValue> = on.iter().map(|n| HookValue::Str(n.clone())).collect();
            self.cache().delete_multiple(&keys, &group)?;
            self.cache().delete(&alloptions_key, &group)?;
        } else if !off.is_empty() {
            let mut all = self.load_alloptions(true)?.read().into_owned();
            for name in &off {
                unset_key(&mut all, name);
            }
            self.cache().set(&alloptions_key, all, &group, 0)?;
        }
        Ok(results)
    }

    /// `wp_set_options_autoload( $options, $autoload )`.
    pub fn set_options_autoload(
        &self,
        names: &[String],
        autoload: HookValue,
    ) -> Result<Vec<(String, bool)>, OptionsError> {
        self.set_option_autoload_values(
            names
                .iter()
                .map(|n| (n.clone(), autoload.clone()))
                .collect(),
        )
    }

    /// `wp_set_option_autoload( $option, $autoload )`.
    pub fn set_option_autoload(
        &self,
        name: &str,
        autoload: HookValue,
    ) -> Result<bool, OptionsError> {
        let results = self.set_option_autoload_values(vec![(name.to_string(), autoload)])?;
        Ok(results
            .into_iter()
            .find(|(n, _)| key_of(n) == key_of(name))
            .is_some_and(|(_, changed)| changed))
    }
}
