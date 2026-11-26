//! Transients and site transients (`option.php` lines 1381–1690 and 2512–2735): the object cache when it is
//! persistent (or while installing), else options (local) or site options (site) with a timeout row.

use std::rc::Rc;

use speedyspec_data::DbError;
use speedyspec_wp_hooks::WpHookApi;
use speedyspec_wp_hooks_contracts::{HookName, HookValue};
use speedyspec_wp_object_cache::{CurrentObjectCache, ObjectCache};

use crate::api::{isset, truthy};
use crate::php_cast::less_than;
use crate::{
    Clock, ExternalObjectCache, Installing, Multisite, NetworkOptionsApi, OptionsApi, OptionsError,
    php_int,
};

/// Which transients a purge removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransientPrefix {
    Transient,
    SiteTransient,
}

impl TransientPrefix {
    pub fn value(&self) -> &'static str {
        match self {
            TransientPrefix::Transient => "_transient_",
            TransientPrefix::SiteTransient => "_site_transient_",
        }
    }

    pub fn timeout(&self) -> &'static str {
        match self {
            TransientPrefix::Transient => "_transient_timeout_",
            TransientPrefix::SiteTransient => "_site_transient_timeout_",
        }
    }
}

/// `delete_expired_transients()`'s statements: expired pairs removed from `wp_options` or `wp_sitemeta`.
pub trait ExpiredTransients {
    fn purge_options(&self, prefix: TransientPrefix, now: i64) -> Result<u64, DbError>;
    fn purge_sitemeta(&self, now: i64) -> Result<u64, DbError>;
}

pub struct TransientPorts {
    pub external: Rc<dyn ExternalObjectCache>,
    pub installing: Rc<dyn Installing>,
    pub multisite: Rc<dyn Multisite>,
    pub clock: Rc<dyn Clock>,
    pub expired: Rc<dyn ExpiredTransients>,
}

/// The site transients WordPress never times out on read.
const NO_TIMEOUT: [&str; 3] = ["update_core", "update_plugins", "update_themes"];

fn s(v: &str) -> HookValue {
    HookValue::Str(v.to_string())
}

fn is_false(v: &HookValue) -> bool {
    matches!(v.read().as_ref(), HookValue::Bool(false))
}

/// WordPress's transients API, one method per function.
pub struct TransientsApi {
    options: Rc<OptionsApi>,
    network: Rc<NetworkOptionsApi>,
    cache: Rc<CurrentObjectCache>,
    hooks: Rc<WpHookApi>,
    ports: TransientPorts,
}

impl TransientsApi {
    pub fn new(
        options: Rc<OptionsApi>,
        network: Rc<NetworkOptionsApi>,
        cache: Rc<CurrentObjectCache>,
        hooks: Rc<WpHookApi>,
        ports: TransientPorts,
    ) -> TransientsApi {
        TransientsApi {
            options,
            network,
            cache,
            hooks,
            ports,
        }
    }

    /// The current object cache, `site-transient` (and `site-options`) global.
    fn cache(&self) -> Rc<ObjectCache> {
        let cache = self.cache.current();
        cache.add_global_groups(&[s("site-options"), s("site-transient")]);
        cache
    }

    /// `wp_using_ext_object_cache() || wp_installing()`.
    fn in_cache(&self) -> bool {
        self.ports.external.using() || self.ports.installing.installing()
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

    /// `do_action_deprecated( $hook, $args, '6.8.0', $replacement )`.
    fn deprecated_action(
        &self,
        hook: &str,
        args: Vec<HookValue>,
        replacement: &str,
    ) -> Result<(), OptionsError> {
        self.hooks.do_action_deprecated(
            &HookName::from(hook),
            args,
            &s("6.8.0"),
            &s(replacement),
            &s(""),
        )?;
        Ok(())
    }

    fn timeout_at(&self, expiration: &HookValue) -> HookValue {
        HookValue::Int(self.ports.clock.now().saturating_add(php_int(expiration)))
    }

    /// `get_transient( $transient )`.
    pub fn get_transient(&self, name: &str) -> Result<HookValue, OptionsError> {
        let pre = self.filter(
            &format!("pre_transient_{name}"),
            HookValue::Bool(false),
            || vec![s(name)],
        )?;
        if !is_false(&pre) {
            return Ok(pre);
        }
        let value = if self.in_cache() {
            self.cache()
                .get(&s(name), &s("transient"), false)?
                .unwrap_or(HookValue::Bool(false))
        } else {
            let option = format!("_transient_{name}");
            let mut value = None;
            if !self.ports.installing.installing() {
                let alloptions = self.options.load_alloptions(false)?;
                if isset(&alloptions.read(), &option).is_none() {
                    let timeout = format!("_transient_timeout_{name}");
                    self.options
                        .prime_option_caches(&[option.clone(), timeout.clone()])?;
                    let at = self.options.get_option(&timeout, None)?;
                    if !is_false(&at) && less_than(&at.read(), self.ports.clock.now()) {
                        self.options.delete_option(&option)?;
                        self.options.delete_option(&timeout)?;
                        value = Some(HookValue::Bool(false));
                    }
                }
            }
            match value {
                Some(v) => v,
                None => self.options.get_option(&option, None)?,
            }
        };
        self.filter(&format!("transient_{name}"), value, || vec![s(name)])
    }

    /// `set_transient( $transient, $value, $expiration )`.
    pub fn set_transient(
        &self,
        name: &str,
        value: HookValue,
        expiration: &HookValue,
    ) -> Result<bool, OptionsError> {
        let cast = HookValue::Int(php_int(expiration));
        let value = self.filter(&format!("pre_set_transient_{name}"), value, || {
            vec![cast.clone(), s(name)]
        })?;
        let expiration = self.filter(&format!("expiration_of_transient_{name}"), cast, || {
            vec![value.clone(), s(name)]
        })?;
        let result = if self.in_cache() {
            self.cache().set(
                &s(name),
                value.clone(),
                &s("transient"),
                php_int(&expiration),
            )?
        } else {
            let timeout = format!("_transient_timeout_{name}");
            let option = format!("_transient_{name}");
            let (o, deprecated) = (&self.options, s(""));
            o.prime_option_caches(&[option.clone(), timeout.clone()])?;
            if is_false(&o.get_option(&option, None)?) {
                let mut autoload = true;
                if truthy(&expiration) {
                    autoload = false;
                    o.add_option(
                        &timeout,
                        self.timeout_at(&expiration),
                        deprecated.clone(),
                        HookValue::Bool(false),
                    )?;
                }
                o.add_option(
                    &option,
                    value.clone(),
                    deprecated,
                    HookValue::Bool(autoload),
                )?
            } else {
                // A transient given a timeout after it had none is made again, so it is no longer autoloaded
                // (ticket 22807).
                let mut result = None;
                if truthy(&expiration) {
                    if is_false(&o.get_option(&timeout, None)?) {
                        o.delete_option(&option)?;
                        o.add_option(
                            &timeout,
                            self.timeout_at(&expiration),
                            deprecated.clone(),
                            HookValue::Bool(false),
                        )?;
                        result = Some(o.add_option(
                            &option,
                            value.clone(),
                            deprecated,
                            HookValue::Bool(false),
                        )?);
                    } else {
                        o.update_option(&timeout, self.timeout_at(&expiration), HookValue::Null)?;
                    }
                }
                match result {
                    Some(r) => r,
                    None => o.update_option(&option, value.clone(), HookValue::Null)?,
                }
            }
        };
        if result {
            self.action(&format!("set_transient_{name}"), || {
                vec![value.clone(), expiration.clone(), s(name)]
            })?;
            self.action("set_transient", || {
                vec![s(name), value.clone(), expiration.clone()]
            })?;
            self.deprecated_action(
                "setted_transient",
                vec![s(name), value, expiration],
                "set_transient",
            )?;
        }
        Ok(result)
    }

    /// `delete_transient( $transient )`.
    pub fn delete_transient(&self, name: &str) -> Result<bool, OptionsError> {
        self.action(&format!("delete_transient_{name}"), || vec![s(name)])?;
        let result = if self.in_cache() {
            self.cache().delete(&s(name), &s("transient"))?
        } else {
            let result = self.options.delete_option(&format!("_transient_{name}"))?;
            if result {
                self.options
                    .delete_option(&format!("_transient_timeout_{name}"))?;
            }
            result
        };
        if result {
            self.action("deleted_transient", || vec![s(name)])?;
        }
        Ok(result)
    }

    fn get_site_option(&self, name: &str) -> Result<HookValue, OptionsError> {
        self.network
            .get_network_option(&HookValue::Null, name, HookValue::Bool(false))
    }

    /// `get_site_transient( $transient )`.
    pub fn get_site_transient(&self, name: &str) -> Result<HookValue, OptionsError> {
        let pre = self.filter(
            &format!("pre_site_transient_{name}"),
            HookValue::Bool(false),
            || vec![s(name)],
        )?;
        if !is_false(&pre) {
            return Ok(pre);
        }
        let value = if self.in_cache() {
            self.cache()
                .get(&s(name), &s("site-transient"), false)?
                .unwrap_or(HookValue::Bool(false))
        } else {
            let option = format!("_site_transient_{name}");
            let mut value = None;
            if !NO_TIMEOUT.contains(&name) {
                let timeout = format!("_site_transient_timeout_{name}");
                self.network
                    .prime_site_option_caches(&[option.clone(), timeout.clone()])?;
                let at = self.get_site_option(&timeout)?;
                if !is_false(&at) && less_than(&at.read(), self.ports.clock.now()) {
                    self.network
                        .delete_network_option(&HookValue::Null, &option)?;
                    self.network
                        .delete_network_option(&HookValue::Null, &timeout)?;
                    value = Some(HookValue::Bool(false));
                }
            }
            match value {
                Some(v) => v,
                None => self.get_site_option(&option)?,
            }
        };
        self.filter(&format!("site_transient_{name}"), value, || vec![s(name)])
    }

    /// `set_site_transient( $transient, $value, $expiration )`.
    pub fn set_site_transient(
        &self,
        name: &str,
        value: HookValue,
        expiration: &HookValue,
    ) -> Result<bool, OptionsError> {
        let value = self.filter(&format!("pre_set_site_transient_{name}"), value, || {
            vec![s(name)]
        })?;
        let cast = HookValue::Int(php_int(expiration));
        let expiration = self.filter(
            &format!("expiration_of_site_transient_{name}"),
            cast,
            || vec![value.clone(), s(name)],
        )?;
        let network = HookValue::Null;
        let result = if self.in_cache() {
            self.cache().set(
                &s(name),
                value.clone(),
                &s("site-transient"),
                php_int(&expiration),
            )?
        } else {
            let timeout = format!("_site_transient_timeout_{name}");
            let option = format!("_site_transient_{name}");
            self.network
                .prime_site_option_caches(&[option.clone(), timeout.clone()])?;
            if is_false(&self.get_site_option(&option)?) {
                if truthy(&expiration) {
                    self.network.add_network_option(
                        &network,
                        &timeout,
                        self.timeout_at(&expiration),
                    )?;
                }
                self.network
                    .add_network_option(&network, &option, value.clone())?
            } else {
                if truthy(&expiration) {
                    self.network.update_network_option(
                        &network,
                        &timeout,
                        self.timeout_at(&expiration),
                    )?;
                }
                self.network
                    .update_network_option(&network, &option, value.clone())?
            }
        };
        if result {
            self.action(&format!("set_site_transient_{name}"), || {
                vec![value.clone(), expiration.clone(), s(name)]
            })?;
            self.action("set_site_transient", || {
                vec![s(name), value.clone(), expiration.clone()]
            })?;
            self.deprecated_action(
                "setted_site_transient",
                vec![s(name), value, expiration],
                "set_site_transient",
            )?;
        }
        Ok(result)
    }

    /// `delete_site_transient( $transient )`.
    pub fn delete_site_transient(&self, name: &str) -> Result<bool, OptionsError> {
        self.action(&format!("delete_site_transient_{name}"), || vec![s(name)])?;
        let network = HookValue::Null;
        let result = if self.in_cache() {
            self.cache().delete(&s(name), &s("site-transient"))?
        } else {
            let result = self
                .network
                .delete_network_option(&network, &format!("_site_transient_{name}"))?;
            if result {
                self.network
                    .delete_network_option(&network, &format!("_site_transient_timeout_{name}"))?;
            }
            result
        };
        if result {
            self.action("deleted_site_transient", || vec![s(name)])?;
        }
        Ok(result)
    }

    /// `delete_expired_transients( $force_db )`.
    pub fn delete_expired_transients(&self, force_db: bool) -> Result<(), OptionsError> {
        if !force_db && self.ports.external.using() {
            return Ok(());
        }
        let now = self.ports.clock.now();
        let db = |e: DbError| OptionsError::Db(e.to_string());
        self.ports
            .expired
            .purge_options(TransientPrefix::Transient, now)
            .map_err(db)?;
        let site = &self.ports.multisite;
        if !site.is_multisite() {
            self.ports
                .expired
                .purge_options(TransientPrefix::SiteTransient, now)
                .map_err(db)?;
        } else if site.is_main_site() && site.is_main_network() {
            self.ports.expired.purge_sitemeta(now).map_err(db)?;
        }
        Ok(())
    }
}
