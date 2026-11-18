//! The release state: what is active, kept, failed and pinned per app. Persisted as `state.json`.

use std::collections::BTreeMap;

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::AppId;

/// Every app's release state. Every field defaults, and unknown fields are ignored, so later builds can add fields
/// without breaking older state files.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReleaseState {
    /// Increases on every write.
    pub revision: u64,
    pub apps: BTreeMap<AppId, AppState>,
}

/// One app's releases.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    pub active: Option<Version>,
    /// The version that was active before the last switch: the default rollback target.
    pub previous: Option<Version>,
    /// Kept releases in install order, oldest first.
    pub kept: Vec<Version>,
    /// Versions that failed to prepare or install, with the reason.
    pub failed: BTreeMap<Version, String>,
    /// While set, auto-update leaves the app alone.
    pub pinned: Option<Version>,
}

impl ReleaseState {
    /// The app's state, created empty if it has none yet.
    pub fn app_mut(&mut self, app: &AppId) -> &mut AppState {
        self.apps.entry(app.clone()).or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips_and_tolerates_unknown_and_missing_fields() {
        let empty: ReleaseState = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, ReleaseState::default());

        let mut state = ReleaseState::default();
        let admin = AppId::parse("admin").unwrap();
        let app = state.app_mut(&admin);
        app.active = Some(Version::new(1, 0, 0));
        app.kept = vec![Version::new(1, 0, 0)];
        app.failed.insert(Version::new(0, 9, 0), "bad".to_string());
        state.revision = 3;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<ReleaseState>(&json).unwrap(), state);

        let future: ReleaseState = serde_json::from_str(
            r#"{"revision":1,"epoch":7,"apps":{"admin":{"active":"1.0.0","trial":true}}}"#,
        )
        .unwrap();
        assert_eq!(future.apps[&admin].active, Some(Version::new(1, 0, 0)));
        assert!(future.apps[&admin].kept.is_empty());
    }
}
