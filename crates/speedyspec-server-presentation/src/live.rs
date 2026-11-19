//! The live handle of every app, each swappable in one step.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use arc_swap::ArcSwap;
use speedyspec_server_domain::AppId;

use crate::handlers::{AppHandle, StaticApp};

/// One swappable handle per configured app. A request takes the handle once and answers entirely from it, so a
/// switch never mixes two releases in one response.
pub struct LiveApps {
    apps: HashMap<AppId, ArcSwap<AppHandle>>,
}

impl LiveApps {
    /// The set of apps is fixed here.
    pub fn new(initial: HashMap<AppId, AppHandle>) -> LiveApps {
        LiveApps {
            apps: initial
                .into_iter()
                .map(|(app, handle)| (app, ArcSwap::from_pointee(handle)))
                .collect(),
        }
    }

    pub fn get(&self, app: &AppId) -> Option<Arc<AppHandle>> {
        self.apps.get(app).map(ArcSwap::load_full)
    }

    /// Replaces an app's handle and returns the old one.
    ///
    /// # Panics
    ///
    /// If `app` was not given to `new`.
    pub fn swap(&self, app: &AppId, handle: AppHandle) -> Option<Arc<AppHandle>> {
        let slot = self
            .apps
            .get(app)
            .unwrap_or_else(|| panic!("{app} is not a configured app"));
        Some(slot.swap(Arc::new(handle)))
    }

    /// Drops every static fallback whose grace period has ended. Uses compare-and-swap, so a switch that lands at
    /// the same moment is never overwritten.
    pub fn drop_expired_fallbacks(&self, now: Instant) {
        for slot in self.apps.values() {
            slot.rcu(|handle| match &**handle {
                AppHandle::Static(app)
                    if app
                        .fallback
                        .as_ref()
                        .is_some_and(|fallback| now >= fallback.until) =>
                {
                    Arc::new(AppHandle::Static(StaticApp {
                        current: app.current.clone(),
                        fallback: None,
                    }))
                }
                _ => handle.clone(),
            });
        }
    }

    /// Every configured app.
    pub fn apps(&self) -> impl Iterator<Item = &AppId> {
        self.apps.keys()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_swap_returns_old_and_serves_new() {
        let app = AppId::parse("blog").unwrap();
        let live = LiveApps::new(
            [(app.clone(), AppHandle::NotInstalled)]
                .into_iter()
                .collect(),
        );
        let held = live.get(&app).unwrap();
        let old = live.swap(&app, AppHandle::AdminApiPlaceholder).unwrap();
        assert!(matches!(*old, AppHandle::NotInstalled));
        assert!(
            matches!(*held, AppHandle::NotInstalled),
            "a request already holding the old handle keeps it"
        );
        assert!(matches!(
            *live.get(&app).unwrap(),
            AppHandle::AdminApiPlaceholder
        ));
        assert!(live.get(&AppId::parse("other").unwrap()).is_none());
    }

    #[test]
    fn expired_fallbacks_are_dropped_and_live_ones_kept() {
        use std::time::{Duration, Instant};

        use speedyspec_server_domain::StaticSection;
        use speedyspec_server_static::StaticSet;

        use crate::handlers::{Fallback, StaticApp};

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("dist")).unwrap();
        let section = StaticSection {
            root: "dist".to_string(),
            not_found: None,
            cache: Vec::new(),
        };
        let set = Arc::new(StaticSet::build(dir.path(), &section, u64::MAX).unwrap());
        let now = Instant::now();
        let app = |until: Instant| {
            AppHandle::Static(StaticApp {
                current: set.clone(),
                fallback: Some(Fallback {
                    set: set.clone(),
                    until,
                }),
            })
        };
        let (old, fresh) = (AppId::parse("old").unwrap(), AppId::parse("fresh").unwrap());
        let live = LiveApps::new(
            [
                (old.clone(), app(now - Duration::from_secs(1))),
                (fresh.clone(), app(now + Duration::from_secs(60))),
            ]
            .into_iter()
            .collect(),
        );
        live.drop_expired_fallbacks(now);
        let fallback = |id: &AppId| match &*live.get(id).unwrap() {
            AppHandle::Static(app) => app.fallback.is_some(),
            _ => unreachable!(),
        };
        assert!(!fallback(&old));
        assert!(fallback(&fresh));
    }
}
