use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_firewall::{
    BanStore, CounterStore, FirewallLog, FirewallSettings, RawSettings, SettingsSource,
};

use crate::firewall::{Firewall, FirewallScope};
use crate::login::LoginGuard;

struct Built {
    raw: RawSettings,
    settings: Rc<FirewallSettings>,
    firewall: Rc<Firewall>,
}

/// Loads the settings on every call and rebuilds the firewall only when they changed (memoization), logging each
/// skipped settings line once per rebuild.
pub struct FirewallCache {
    source: Rc<dyn SettingsSource>,
    scope: FirewallScope,
    counters: Rc<dyn CounterStore>,
    bans: Rc<dyn BanStore>,
    log: Rc<dyn FirewallLog>,
    built: RefCell<Option<Built>>,
}

impl FirewallCache {
    pub fn new(
        source: Rc<dyn SettingsSource>,
        scope: FirewallScope,
        counters: Rc<dyn CounterStore>,
        bans: Rc<dyn BanStore>,
        log: Rc<dyn FirewallLog>,
    ) -> FirewallCache {
        FirewallCache {
            source,
            scope,
            counters,
            bans,
            log,
            built: RefCell::new(None),
        }
    }

    pub fn current(&self) -> Option<Rc<Firewall>> {
        self.refresh().map(|(_, firewall)| firewall)
    }

    pub fn settings(&self) -> Option<Rc<FirewallSettings>> {
        self.refresh().map(|(settings, _)| settings)
    }

    /// `None` when the settings cannot be loaded, the firewall is disabled or login protection is off.
    pub fn login_guard(&self) -> Option<LoginGuard> {
        let (settings, _) = self.refresh()?;
        if !settings.enabled {
            return None;
        }
        let policy = settings.login.clone()?;
        Some(LoginGuard::new(
            policy,
            self.counters.clone(),
            self.bans.clone(),
        ))
    }

    fn refresh(&self) -> Option<(Rc<FirewallSettings>, Rc<Firewall>)> {
        let raw = match self.source.load() {
            Ok(raw) => raw,
            Err(error) => {
                self.log.warn(&format!("settings: {error}"));
                return None;
            }
        };
        let mut built = self.built.borrow_mut();
        if built.as_ref().is_none_or(|b| b.raw != raw) {
            let (settings, issues) = FirewallSettings::from_raw(&raw);
            for issue in &issues {
                self.log.warn(&format!(
                    "settings: {} line {}: {}",
                    issue.setting, issue.line, issue.message
                ));
            }
            let firewall = Firewall::from_settings(
                &settings,
                self.scope,
                self.counters.clone(),
                self.bans.clone(),
                self.log.clone(),
            );
            *built = Some(Built {
                raw,
                settings: Rc::new(settings),
                firewall: Rc::new(firewall),
            });
        }
        let current = built.as_ref().expect("built above");
        Some((current.settings.clone(), current.firewall.clone()))
    }
}
