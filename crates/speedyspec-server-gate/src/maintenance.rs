//! Per-app maintenance: on while any of its sources is on.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use speedyspec_server_domain::AppId;

/// What can put an app into maintenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MaintenanceSource {
    /// The admin API's site state says so.
    Site,
    /// A switch to a release that migrates, or one whose migration failed.
    Upgrade,
    /// An operator's `maintenance on`.
    Operator,
    /// The app's health probes are failing.
    Health,
}

/// Every app's maintenance sources.
#[derive(Debug, Default)]
pub struct MaintenanceSet {
    sources: Mutex<BTreeMap<AppId, BTreeSet<MaintenanceSource>>>,
}

impl MaintenanceSet {
    pub fn set(&self, app: &AppId, source: MaintenanceSource, on: bool) {
        let mut sources = self.sources.lock().unwrap();
        let entry = sources.entry(app.clone()).or_default();
        if on {
            entry.insert(source);
        } else {
            entry.remove(&source);
        }
    }

    pub fn is_on(&self, app: &AppId) -> bool {
        self.sources
            .lock()
            .unwrap()
            .get(app)
            .is_some_and(|s| !s.is_empty())
    }

    pub fn sources(&self, app: &AppId) -> BTreeSet<MaintenanceSource> {
        self.sources
            .lock()
            .unwrap()
            .get(app)
            .cloned()
            .unwrap_or_default()
    }

    /// The apps in maintenance because of `source`.
    pub fn in_maintenance_by(&self, source: MaintenanceSource) -> Vec<AppId> {
        self.sources
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, sources)| sources.contains(&source))
            .map(|(app, _)| app.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> AppId {
        AppId::parse(value).unwrap()
    }

    #[test]
    fn on_while_any_source_is_on() {
        let set = MaintenanceSet::default();
        let blog = id("blog");
        set.set(&blog, MaintenanceSource::Operator, true);
        set.set(&blog, MaintenanceSource::Health, true);
        assert!(set.is_on(&blog));
        set.set(&blog, MaintenanceSource::Health, false);
        assert!(set.is_on(&blog));
        assert_eq!(set.sources(&blog), [MaintenanceSource::Operator].into());
        set.set(&blog, MaintenanceSource::Operator, false);
        assert!(!set.is_on(&blog));
    }

    #[test]
    fn apps_are_independent() {
        let set = MaintenanceSet::default();
        set.set(&id("blog"), MaintenanceSource::Health, true);
        assert!(set.is_on(&id("blog")));
        assert!(!set.is_on(&id("frontend-api")));
        assert_eq!(
            set.in_maintenance_by(MaintenanceSource::Health),
            vec![id("blog")]
        );
    }
}
