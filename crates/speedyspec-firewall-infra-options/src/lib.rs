//! The firewall's settings, read through the ported options API (Adapter). Each setting is its own
//! `speedyspec_firewall_*` option; a missing one gets its default from [`RawSettings::default`].

use speedyspec_firewall::{FirewallError, RawSettings, SettingsSource};
use speedyspec_wp_options::{FromOptionValue, Options, OptionsError};

const PREFIX: &str = "speedyspec_firewall_";

pub struct OptionsSettingsSource {
    options: Options,
}

impl OptionsSettingsSource {
    pub fn new(options: Options) -> OptionsSettingsSource {
        OptionsSettingsSource { options }
    }

    fn read<T: FromOptionValue>(&self, name: &str, default: T) -> Result<T, FirewallError> {
        let name = format!("{PREFIX}{name}");
        let value = self.options.get::<T>(&name).map_err(|error: OptionsError| {
            FirewallError(format!("reading {name}: {error:?}"))
        })?;
        Ok(value.unwrap_or(default))
    }
}

impl SettingsSource for OptionsSettingsSource {
    fn load(&self) -> Result<RawSettings, FirewallError> {
        let d = RawSettings::default();
        Ok(RawSettings {
            enabled: self.read("enabled", d.enabled)?,
            allow: self.read("allow", d.allow)?,
            block: self.read("block", d.block)?,
            patterns: self.read("patterns", d.patterns)?,
            rate_requests: self.read("rate_requests", d.rate_requests)?,
            rate_window: self.read("rate_window", d.rate_window)?,
            rate_ban: self.read("rate_ban", d.rate_ban)?,
            login_max_failures: self.read("login_max_failures", d.login_max_failures)?,
            login_window: self.read("login_window", d.login_window)?,
            login_lockout: self.read("login_lockout", d.login_lockout)?,
            signin_paths: self.read("signin_paths", d.signin_paths)?,
            block_xmlrpc: self.read("block_xmlrpc", d.block_xmlrpc)?,
            block_user_enumeration: self.read("block_user_enumeration", d.block_user_enumeration)?,
        })
    }
}
