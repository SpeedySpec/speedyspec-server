//! The read path: `get_option`, `wp_load_alloptions`, `wp_prime_option_caches`, `wp_prime_option_caches_by_group`,
//! `get_options` and `wp_protect_special_option`, in `option.php`'s order of checks, hooks, queries and cache calls.

use speedyspec_wp_hooks::{HookValue, Key};

use crate::api::{OptionsApi, isset, key_of, set_key, truthy};
use crate::{OptionName, OptionsError, autoload_values, php_string};

impl OptionsApi {
    /// `get_option( $option, $default_value )`; `default` `None` is a call with one argument.
    pub fn get_option(
        &self,
        name: &str,
        default: Option<HookValue>,
    ) -> Result<HookValue, OptionsError> {
        let Some(option) = OptionName::new(name) else {
            return Ok(HookValue::Bool(false));
        };
        let passed = default.is_some();
        let default = default.unwrap_or(HookValue::Bool(false));
        if let Some(new) = option.deprecated_replacement()
            && !self.d.installing.installing()
        {
            self.d.deprecations.deprecated_argument(
                "get_option",
                "5.5.0",
                &format!("The \"{option}\" option key has been renamed to \"{new}\"."),
            )?;
            return self.get_option(new, Some(default));
        }
        let n = option.as_str();
        let name_value = || HookValue::Str(n.to_string());
        let pre = self.filter(&format!("pre_option_{n}"), HookValue::Bool(false), || {
            vec![name_value(), default.clone()]
        })?;
        let pre = self.filter("pre_option", pre, || vec![name_value(), default.clone()])?;
        if !matches!(pre.read().as_ref(), HookValue::Bool(false)) {
            return Ok(pre);
        }
        if self.d.installing.setup_config() {
            return Ok(HookValue::Bool(false));
        }
        let default_option = |default: HookValue| {
            self.filter(&format!("default_option_{n}"), default, || {
                vec![name_value(), HookValue::Bool(passed)]
            })
        };
        let group = Self::group();
        let value = if !self.d.installing.installing() {
            let alloptions = self.load_alloptions(false)?;
            match isset(&alloptions.read(), n) {
                Some(value) => value,
                None => {
                    let key = HookValue::Str("notoptions".into());
                    let mut notoptions = self
                        .cache()
                        .get(&key, &group, false)?
                        .unwrap_or(HookValue::Bool(false));
                    if !matches!(notoptions.read().as_ref(), HookValue::Array(_)) {
                        notoptions = HookValue::Array(Vec::new());
                        self.cache().set(&key, notoptions.clone(), &group, 0)?;
                    }
                    if isset(&notoptions.read(), n).is_some() {
                        return default_option(default);
                    }
                    match self.cache().get(&name_value(), &group, false)? {
                        Some(value) if !matches!(value, HookValue::Bool(false)) => value,
                        _ => match self.d.repository.value(&option, false)? {
                            Some(stored) => {
                                let value = stored.to_value();
                                self.cache().add(&name_value(), value.clone(), &group, 0)?;
                                value
                            }
                            None => {
                                let mut notoptions = notoptions.read().into_owned();
                                set_key(&mut notoptions, n, HookValue::Bool(true));
                                self.cache().set(&key, notoptions, &group, 0)?;
                                return default_option(default);
                            }
                        },
                    }
                }
            }
        } else {
            match self.d.repository.value(&option, true)? {
                Some(stored) => stored.to_value(),
                None => return default_option(default),
            }
        };
        if n == "home" && matches!(&value, HookValue::Str(s) if s.is_empty()) {
            return self.get_option("siteurl", None);
        }
        let value = if ["siteurl", "home", "category_base", "tag_base"].contains(&n) {
            untrailingslashit(&value)
        } else {
            value
        };
        let value = self.d.codec.maybe_unserialize(value)?;
        self.filter(&format!("option_{n}"), value, || vec![name_value()])
    }

    /// `wp_load_alloptions( $force_cache )`.
    pub fn load_alloptions(&self, force_cache: bool) -> Result<HookValue, OptionsError> {
        let pre = self.filter("pre_wp_load_alloptions", HookValue::Null, || {
            vec![HookValue::Bool(force_cache)]
        })?;
        if matches!(pre.read().as_ref(), HookValue::Array(_)) {
            return Ok(pre);
        }
        let key = HookValue::Str("alloptions".into());
        let group = Self::group();
        let cached = if self.caches_alloptions() {
            self.cache()
                .get(&key, &group, force_cache)?
                .unwrap_or(HookValue::Bool(false))
        } else {
            HookValue::Bool(false)
        };
        let alloptions = if truthy(&cached.read()) {
            cached
        } else {
            let values = autoload_values(&self.d.autoload.values_to_autoload()?);
            let mut rows = self.d.repository.autoloaded(&values)?;
            if rows.is_empty() {
                rows = self.d.repository.all()?;
            }
            let mut alloptions = HookValue::Array(Vec::new());
            for (name, value) in rows {
                set_key(&mut alloptions, &name, value.to_value());
            }
            if self.caches_alloptions() {
                alloptions = self.filter("pre_cache_alloptions", alloptions, Vec::new)?;
                // The cache keeps a copy (PHP arrays are values), never the PHP handle a callback returned: a handle
                // written back into an array corrupts it (speedyspec-php's `write` copies a whole zval into a hash
                // bucket, chain link included).
                self.cache()
                    .add(&key, alloptions.read().into_owned(), &group, 0)?;
            }
            alloptions
        };
        self.filter("alloptions", alloptions, Vec::new)
    }

    /// `wp_prime_option_caches( $options )`: one query for the names no cache knows.
    pub fn prime_option_caches(&self, names: &[String]) -> Result<(), OptionsError> {
        let alloptions = self.load_alloptions(false)?.read().into_owned();
        let group = Self::group();
        let keys: Vec<HookValue> = names.iter().map(|n| HookValue::Str(n.clone())).collect();
        let cached = self.cache().get_multiple(&keys, &group, false)?;
        let notoptions_key = HookValue::Str("notoptions".into());
        let mut notoptions = self
            .cache()
            .get(&notoptions_key, &group, false)?
            .map(|v| v.read().into_owned())
            .unwrap_or(HookValue::Bool(false));
        if !matches!(notoptions, HookValue::Array(_)) {
            notoptions = HookValue::Array(Vec::new());
        }
        let cached = HookValue::Array(cached);
        let to_prime: Vec<String> = names
            .iter()
            .filter(|n| {
                isset(&cached, n).is_none_or(|v| matches!(v, HookValue::Bool(false)))
                    && isset(&alloptions, n).is_none()
                    && isset(&notoptions, n).is_none()
            })
            .cloned()
            .collect();
        if to_prime.is_empty() {
            return Ok(());
        }
        let results = self.d.repository.values_of(&to_prime)?;
        let mut found: Vec<(Key, HookValue)> = Vec::new();
        for (name, value) in results {
            let key = key_of(&name);
            match found.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value.to_value(),
                None => found.push((key, value.to_value())),
            }
        }
        let found_count = found.len();
        let found_keys: Vec<Key> = found.iter().map(|(k, _)| k.clone()).collect();
        self.cache().set_multiple(found, &group, 0)?;
        if found_count == to_prime.len() {
            return Ok(());
        }
        let mut update = false;
        for name in to_prime.iter().filter(|n| !found_keys.contains(&key_of(n))) {
            if isset(&notoptions, name).is_none() {
                set_key(&mut notoptions, name, HookValue::Bool(true));
                update = true;
            }
        }
        if update {
            self.cache().set(&notoptions_key, notoptions, &group, 0)?;
        }
        Ok(())
    }

    /// `wp_prime_option_caches_by_group( $option_group )`.
    pub fn prime_option_caches_by_group(&self, group: &str) -> Result<(), OptionsError> {
        match self.d.groups.options_in(group) {
            Some(names) => self.prime_option_caches(&names),
            None => Ok(()),
        }
    }

    /// `get_options( $options )`: an array by name (a repeated name keeps its first place).
    pub fn get_options(&self, names: &[String]) -> Result<Vec<(String, HookValue)>, OptionsError> {
        self.prime_option_caches(names)?;
        let mut result: Vec<(String, HookValue)> = Vec::new();
        for name in names {
            let value = self.get_option(name, None)?;
            match result.iter_mut().find(|(n, _)| key_of(n) == key_of(name)) {
                Some(slot) => slot.1 = value,
                None => result.push((name.clone(), value)),
            }
        }
        Ok(result)
    }

    /// `wp_protect_special_option( $option )`.
    pub fn protect_special_option(&self, name: &str) -> Result<(), OptionsError> {
        self.d.protected.protect(name)
    }
}

/// `untrailingslashit()`: `rtrim( $value, '/\\' )`.
fn untrailingslashit(value: &HookValue) -> HookValue {
    let bytes = php_string(&value.read()).unwrap_or_default();
    let end = bytes
        .iter()
        .rposition(|b| *b != b'/' && *b != b'\\')
        .map_or(0, |i| i + 1);
    match String::from_utf8(bytes[..end].to_vec()) {
        Ok(s) => HookValue::Str(s),
        Err(e) => HookValue::Bytes(e.into_bytes()),
    }
}
