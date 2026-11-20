use std::rc::Rc;

use speedyspec_cache::CachingStore;
use speedyspec_firewall::FirewallLog;
use speedyspec_firewall_app::FirewallCache;
use speedyspec_firewall_infra_cache::{CacheBanStore, CacheCounterStore, SystemClock};
use speedyspec_firewall_infra_options::OptionsSettingsSource;
use speedyspec_wp_hooks::{HookApi, HookError};
use speedyspec_wp_options::Options;

use crate::hooks::register_hooks;
use crate::stage::{CurrentClient, FirewallStage};

/// Writes warnings to stderr, prefixed `speedyspec-firewall: `.
pub struct StderrLog;

impl FirewallLog for StderrLog {
    fn warn(&self, message: &str) {
        eprintln!("speedyspec-firewall: {message}");
    }
}

/// The firewall's composition root (Factory): the one call a runtime worker makes, once, at startup.
pub struct FirewallComposition;

impl FirewallComposition {
    pub fn build(
        options: Options,
        cache: Rc<dyn CachingStore>,
        hooks: &HookApi,
        log: Rc<dyn FirewallLog>,
    ) -> Result<Rc<FirewallStage>, HookError> {
        let counters = Rc::new(CacheCounterStore::new(cache.clone(), Rc::new(SystemClock)));
        let bans = Rc::new(CacheBanStore::new(cache));
        let source = Rc::new(OptionsSettingsSource::new(options));
        let firewall_cache = Rc::new(FirewallCache::new(source, counters, bans, log.clone()));
        let current = CurrentClient::default();
        register_hooks(hooks, firewall_cache.clone(), current.clone(), log.clone())?;
        Ok(Rc::new(FirewallStage::new(firewall_cache, current, log)))
    }
}
