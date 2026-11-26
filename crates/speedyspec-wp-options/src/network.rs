//! Site and network options (`option.php` lines 677–846 and 1935–2500): `wp_sitemeta` on multisite, the options
//! API on a single site, cached in the global `site-options` group under `"{network_id}:{option}"`.

use std::rc::Rc;

use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_hooks_contracts::{HookValue, Key};
use speedyspec_wp_object_cache::{CurrentObjectCache, ObjectCache};

use crate::api::{isset, set_key, truthy, unset_key};
use crate::{
    Installing, Multisite, NetworkMeta, OptionName, OptionsApi, OptionsError, ProtectedOptions,
    Sanitizing, SerializedValue, SitemetaRepository,
};

/// The names `wp_load_core_site_options()` primes.
const CORE_SITE_OPTIONS: [&str; 9] = [
    "site_name",
    "siteurl",
    "active_sitewide_plugins",
    "_site_transient_timeout_theme_roots",
    "_site_transient_theme_roots",
    "site_admins",
    "global_terms_enabled",
    "ms_files_rewriting",
    "WPLANG",
];

const GROUP: &str = "site-options";

/// PHP's `is_numeric()`: a number, or a string holding one (surrounding whitespace allowed).
fn is_numeric(value: &HookValue) -> bool {
    match value {
        HookValue::Int(_) | HookValue::Float(_) => true,
        HookValue::Str(s) => numeric_text(s).is_some(),
        HookValue::Bytes(b) => std::str::from_utf8(b).ok().and_then(numeric_text).is_some(),
        _ => false,
    }
}

fn numeric_text(s: &str) -> Option<f64> {
    let t = s.trim_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    let plain = !digits.is_empty()
        && digits
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        && digits.starts_with(|c: char| c.is_ascii_digit() || c == '.');
    plain.then(|| t.parse::<f64>().ok()).flatten()
}

/// `$network_id` as every network function reads it: truthy and not numeric is refused (`None`); otherwise
/// `(int)`, and 0 is the current network.
pub fn resolve_network_id(raw: &HookValue, multisite: &dyn Multisite) -> Option<i64> {
    if truthy(raw) && !is_numeric(raw) {
        return None;
    }
    match crate::php_int(raw) {
        0 => Some(multisite.network_id() as i64),
        id => Some(id),
    }
}

pub struct NetworkPorts {
    pub multisite: Rc<dyn Multisite>,
    pub installing: Rc<dyn Installing>,
    pub sanitizing: Rc<dyn Sanitizing>,
    pub protected: Rc<dyn ProtectedOptions>,
}

/// WordPress's site and network options, one method per function.
pub struct NetworkOptionsApi {
    options: Rc<OptionsApi>,
    sitemeta: Rc<dyn SitemetaRepository>,
    cache: Rc<CurrentObjectCache>,
    hooks: Rc<HookApi>,
    ports: NetworkPorts,
}

fn s(v: &str) -> HookValue {
    HookValue::Str(v.to_string())
}

fn is_array(v: &HookValue) -> bool {
    matches!(v.read().as_ref(), HookValue::Array(_))
}

fn is_false(v: &HookValue) -> bool {
    matches!(v.read().as_ref(), HookValue::Bool(false))
}

impl NetworkOptionsApi {
    pub fn new(
        options: Rc<OptionsApi>,
        sitemeta: Rc<dyn SitemetaRepository>,
        cache: Rc<CurrentObjectCache>,
        hooks: Rc<HookApi>,
        ports: NetworkPorts,
    ) -> NetworkOptionsApi {
        let api = NetworkOptionsApi {
            options,
            sitemeta,
            cache,
            hooks,
            ports,
        };
        api.cache();
        api
    }

    pub fn hooks(&self) -> &Rc<HookApi> {
        &self.hooks
    }

    /// The current object cache, with `site-options` and `site-transient` global, as `wp_start_object_cache()`
    /// makes them on every request.
    fn cache(&self) -> Rc<ObjectCache> {
        let cache = self.cache.current();
        cache.add_global_groups(&[s(GROUP), s("site-transient")]);
        cache
    }

    fn filter(
        &self,
        hook: &str,
        value: HookValue,
        args: impl FnOnce() -> Vec<HookValue>,
    ) -> Result<HookValue, OptionsError> {
        self.options.filter(hook, value, args)
    }

    fn action(
        &self,
        hook: &str,
        args: impl FnOnce() -> Vec<HookValue>,
    ) -> Result<(), OptionsError> {
        self.options.action(hook, args)
    }

    fn cache_get(&self, key: &str) -> Result<HookValue, OptionsError> {
        Ok(self
            .cache()
            .get(&s(key), &s(GROUP), false)?
            .unwrap_or(HookValue::Bool(false)))
    }

    fn cache_set(&self, key: &str, value: HookValue) -> Result<(), OptionsError> {
        self.cache().set(&s(key), value, &s(GROUP), 0)?;
        Ok(())
    }

    fn sanitize(&self, option: &str, value: HookValue) -> Result<HookValue, OptionsError> {
        match OptionName::new(option) {
            Some(name) => self.ports.sanitizing.sanitize_option(&name, value),
            None => Ok(value),
        }
    }

    fn stored(&self, value: &HookValue) -> Result<SerializedValue, OptionsError> {
        let serialized = self.options.deps().codec.maybe_serialize(value)?;
        SerializedValue::for_insert(&serialized.read())
    }

    fn db(e: speedyspec_data::DbError) -> OptionsError {
        OptionsError::Db(e.to_string())
    }

    /// `get_network_option( $network_id, $option, $default_value )`.
    pub fn get_network_option(
        &self,
        network_id: &HookValue,
        option: &str,
        default: HookValue,
    ) -> Result<HookValue, OptionsError> {
        let Some(id) = resolve_network_id(network_id, &*self.ports.multisite) else {
            return Ok(HookValue::Bool(false));
        };
        let args = || vec![s(option), HookValue::Int(id), default.clone()];
        let pre = self.filter(
            &format!("pre_site_option_{option}"),
            HookValue::Bool(false),
            args,
        )?;
        let pre = self.filter("pre_site_option", pre, args)?;
        if !is_false(&pre) {
            return Ok(pre);
        }
        let notoptions_key = format!("{id}:notoptions");
        let mut notoptions = self.cache_get(&notoptions_key)?;
        let default_site_option = |default: HookValue| {
            self.filter(&format!("default_site_option_{option}"), default, || {
                vec![s(option), HookValue::Int(id)]
            })
        };
        if is_array(&notoptions) && isset(&notoptions.read(), option).is_some() {
            return default_site_option(default);
        }
        let value = if !self.ports.multisite.is_multisite() {
            let default = default_site_option(default)?;
            self.options.get_option(option, Some(default))?
        } else {
            let cache_key = format!("{id}:{option}");
            let cached = self.cache().get(&s(&cache_key), &s(GROUP), false)?;
            match cached {
                Some(value) if !is_false(&value) && !matches!(value, HookValue::Null) => value,
                _ => match self.sitemeta.find(id, option).map_err(Self::db)? {
                    Some(row) => {
                        let value = self
                            .options
                            .deps()
                            .codec
                            .maybe_unserialize(row.value.to_value())?;
                        self.cache_set(&cache_key, value.clone())?;
                        value
                    }
                    None => {
                        if !is_array(&notoptions) {
                            notoptions = HookValue::Array(Vec::new());
                        }
                        let mut updated = notoptions.read().into_owned();
                        set_key(&mut updated, option, HookValue::Bool(true));
                        notoptions = updated.clone();
                        self.cache_set(&notoptions_key, updated)?;
                        default_site_option(default)?
                    }
                },
            }
        };
        if !is_array(&notoptions) {
            self.cache_set(&notoptions_key, HookValue::Array(Vec::new()))?;
        }
        self.filter(&format!("site_option_{option}"), value, || {
            vec![s(option), HookValue::Int(id)]
        })
    }

    /// `add_network_option( $network_id, $option, $value )`.
    pub fn add_network_option(
        &self,
        network_id: &HookValue,
        option: &str,
        value: HookValue,
    ) -> Result<bool, OptionsError> {
        let Some(id) = resolve_network_id(network_id, &*self.ports.multisite) else {
            return Ok(false);
        };
        self.ports.protected.protect(option)?;
        let value = self.filter(&format!("pre_add_site_option_{option}"), value, || {
            vec![s(option), HookValue::Int(id)]
        })?;
        let notoptions_key = format!("{id}:notoptions");
        let (added, value) = if !self.ports.multisite.is_multisite() {
            let added =
                self.options
                    .add_option(option, value.clone(), s(""), HookValue::Bool(false))?;
            (added, value)
        } else {
            let cache_key = format!("{id}:{option}");
            // Make sure the option does not already exist.
            let notoptions = self.cache_get(&notoptions_key)?;
            if (!is_array(&notoptions) || isset(&notoptions.read(), option).is_none())
                && !is_false(&self.get_network_option(
                    &HookValue::Int(id),
                    option,
                    HookValue::Bool(false),
                )?)
            {
                return Ok(false);
            }
            let value = self.sanitize(option, value)?;
            let meta = NetworkMeta {
                network_id: id,
                key: option.to_string(),
                value: self.stored(&value)?,
            };
            if !self.sitemeta.add(meta).map_err(Self::db)? {
                return Ok(false);
            }
            self.cache_set(&cache_key, value.clone())?;
            // Again: it must be fresh.
            let notoptions = self.cache_get(&notoptions_key)?;
            if is_array(&notoptions) && isset(&notoptions.read(), option).is_some() {
                let mut updated = notoptions.read().into_owned();
                unset_key(&mut updated, option);
                self.cache_set(&notoptions_key, updated)?;
            }
            (true, value)
        };
        if !added {
            return Ok(false);
        }
        let args = || vec![s(option), value.clone(), HookValue::Int(id)];
        self.action(&format!("add_site_option_{option}"), args)?;
        self.action("add_site_option", args)?;
        Ok(true)
    }

    /// `delete_network_option( $network_id, $option )`.
    pub fn delete_network_option(
        &self,
        network_id: &HookValue,
        option: &str,
    ) -> Result<bool, OptionsError> {
        let Some(id) = resolve_network_id(network_id, &*self.ports.multisite) else {
            return Ok(false);
        };
        let args = || vec![s(option), HookValue::Int(id)];
        self.action(&format!("pre_delete_site_option_{option}"), args)?;
        let deleted = if !self.ports.multisite.is_multisite() {
            self.options.delete_option(option)?
        } else {
            if !self.sitemeta.exists(id, option).map_err(Self::db)? {
                return Ok(false);
            }
            self.cache()
                .delete(&s(&format!("{id}:{option}")), &s(GROUP))?;
            let deleted = self.sitemeta.remove(id, option).map_err(Self::db)? > 0;
            if deleted {
                let notoptions_key = format!("{id}:notoptions");
                let mut notoptions = self.cache_get(&notoptions_key)?.read().into_owned();
                set_key(&mut notoptions, option, HookValue::Bool(true));
                self.cache_set(&notoptions_key, notoptions)?;
            }
            deleted
        };
        if !deleted {
            return Ok(false);
        }
        self.action(&format!("delete_site_option_{option}"), args)?;
        self.action("delete_site_option", args)?;
        Ok(true)
    }

    /// `update_network_option( $network_id, $option, $value )`.
    pub fn update_network_option(
        &self,
        network_id: &HookValue,
        option: &str,
        value: HookValue,
    ) -> Result<bool, OptionsError> {
        let Some(id) = resolve_network_id(network_id, &*self.ports.multisite) else {
            return Ok(false);
        };
        self.ports.protected.protect(option)?;
        let network = HookValue::Int(id);
        let old = self.get_network_option(&network, option, HookValue::Bool(false))?;
        let value = self.filter(&format!("pre_update_site_option_{option}"), value, || {
            vec![old.clone(), s(option), network.clone()]
        })?;
        let codec = &self.options.deps().codec;
        if value == old
            || codec.maybe_serialize(&value)?.read() == codec.maybe_serialize(&old)?.read()
        {
            return Ok(false);
        }
        if is_false(&old) {
            return self.add_network_option(&network, option, value);
        }
        let notoptions_key = format!("{id}:notoptions");
        let notoptions = self.cache_get(&notoptions_key)?;
        if is_array(&notoptions) && isset(&notoptions.read(), option).is_some() {
            let mut updated = notoptions.read().into_owned();
            unset_key(&mut updated, option);
            self.cache_set(&notoptions_key, updated)?;
        }
        let (updated, value) = if !self.ports.multisite.is_multisite() {
            (
                self.options
                    .update_option(option, value.clone(), HookValue::Bool(false))?,
                value,
            )
        } else {
            let value = self.sanitize(option, value)?;
            let meta = NetworkMeta {
                network_id: id,
                key: option.to_string(),
                value: self.stored(&value)?,
            };
            let updated = self.sitemeta.update(meta).map_err(Self::db)? > 0;
            if updated {
                self.cache_set(&format!("{id}:{option}"), value.clone())?;
            }
            (updated, value)
        };
        if !updated {
            return Ok(false);
        }
        let args = || vec![s(option), value.clone(), old.clone(), network.clone()];
        self.action(&format!("update_site_option_{option}"), args)?;
        self.action("update_site_option", args)?;
        Ok(true)
    }

    /// `wp_prime_network_option_caches( $network_id, $options )`.
    pub fn prime_network_option_caches(
        &self,
        network_id: &HookValue,
        options: &[String],
    ) -> Result<(), OptionsError> {
        if self.ports.installing.installing() {
            return Ok(());
        }
        if !self.ports.multisite.is_multisite() {
            return self.options.prime_option_caches(options);
        }
        let Some(id) = resolve_network_id(network_id, &*self.ports.multisite) else {
            return Ok(());
        };
        let keys: Vec<HookValue> = options.iter().map(|o| s(&format!("{id}:{o}"))).collect();
        let cached = self.cache().get_multiple(&keys, &s(GROUP), false)?;
        let notoptions_key = format!("{id}:notoptions");
        let mut notoptions = self.cache_get(&notoptions_key)?;
        if !is_array(&notoptions) {
            notoptions = HookValue::Array(Vec::new());
        }
        let to_prime: Vec<String> = options
            .iter()
            .filter(|o| {
                let key = Key::Str(format!("{id}:{o}"));
                let hit = cached.iter().find(|(k, _)| *k == key).map(|(_, v)| v);
                hit.is_none_or(|v| is_false(v) || matches!(v, HookValue::Null))
                    && isset(&notoptions.read(), o).is_none()
            })
            .cloned()
            .collect();
        if to_prime.is_empty() {
            return Ok(());
        }
        let found = self.sitemeta.find_many(id, &to_prime).map_err(Self::db)?;
        let mut data = Vec::with_capacity(found.len());
        for row in &found {
            let value = self
                .options
                .deps()
                .codec
                .maybe_unserialize(row.value.to_value())?;
            data.push((Key::Str(format!("{id}:{}", row.key)), value));
        }
        self.cache().set_multiple(data, &s(GROUP), 0)?;
        if found.len() == to_prime.len() {
            return Ok(());
        }
        let mut updated = notoptions.read().into_owned();
        let mut changed = false;
        for name in to_prime
            .iter()
            .filter(|n| !found.iter().any(|f| &f.key == *n))
        {
            if isset(&updated, name).is_none() {
                set_key(&mut updated, name, HookValue::Bool(true));
                changed = true;
            }
        }
        if changed {
            self.cache_set(&notoptions_key, updated)?;
        }
        Ok(())
    }

    /// `wp_prime_site_option_caches( $options )`.
    pub fn prime_site_option_caches(&self, options: &[String]) -> Result<(), OptionsError> {
        self.prime_network_option_caches(&HookValue::Null, options)
    }

    /// `wp_load_core_site_options( $network_id )`.
    pub fn load_core_site_options(&self, network_id: &HookValue) -> Result<(), OptionsError> {
        if !self.ports.multisite.is_multisite() || self.ports.installing.installing() {
            return Ok(());
        }
        let names: Vec<String> = CORE_SITE_OPTIONS.iter().map(|n| n.to_string()).collect();
        self.prime_network_option_caches(network_id, &names)
    }
}
