//! The live handle of every app, each swappable in one step.

use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use speedyspec_server_domain::AppId;

use crate::handlers::AppHandle;

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
        let old = live.swap(&app, AppHandle::PhpPlaceholder).unwrap();
        assert!(matches!(*old, AppHandle::NotInstalled));
        assert!(
            matches!(*held, AppHandle::NotInstalled),
            "a request already holding the old handle keeps it"
        );
        assert!(matches!(
            *live.get(&app).unwrap(),
            AppHandle::PhpPlaceholder
        ));
        assert!(live.get(&AppId::parse("other").unwrap()).is_none());
    }
}
