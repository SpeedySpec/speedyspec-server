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
    /// The server binary's trial and confirmation, kept apart from `apps[server]`, whose `kept` lists the kept binaries.
    pub server: ServerState,
    /// A switch that includes the server, recorded before the new generation starts.
    pub pending_group: Option<PendingGroup>,
}

/// Which server binary is confirmed, which is on trial, and which failed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerState {
    /// The binary a crashed or failed generation falls back to.
    pub confirmed: Option<Version>,
    pub trial: Option<Version>,
    pub failed: BTreeMap<Version, String>,
}

/// Every member of a switch that includes the server, with its version before and its target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingGroup {
    /// (previous, target) of the server.
    pub server: (Version, Version),
    /// (app, previous, target) of every other member.
    #[serde(default)]
    pub apps: Vec<(AppId, Option<Version>, Version)>,
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

    #[test]
    fn plan2_state_file_still_loads() {
        let state: ReleaseState = serde_json::from_str(
            r#"{"revision":4,"apps":{"admin":{"active":"1.0.0","previous":null,"kept":["1.0.0"],"failed":{},"pinned":null}}}"#,
        )
        .unwrap();
        assert_eq!(state.server, ServerState::default());
        assert_eq!(state.pending_group, None);
    }

    #[test]
    fn server_state_round_trips() {
        let mut state = ReleaseState::default();
        state.server.confirmed = Some(Version::new(0, 1, 0));
        state.server.trial = Some(Version::new(0, 2, 0));
        state
            .server
            .failed
            .insert(Version::new(0, 1, 5), "trial failed".to_string());
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<ReleaseState>(&json).unwrap(), state);
    }

    #[test]
    fn pending_group_round_trips() {
        let mut state = ReleaseState::default();
        state.pending_group = Some(PendingGroup {
            server: (Version::new(0, 1, 0), Version::new(0, 2, 0)),
            apps: vec![(
                AppId::parse("admin").unwrap(),
                Some(Version::new(1, 0, 0)),
                Version::new(2, 0, 0),
            )],
        });
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<ReleaseState>(&json).unwrap(), state);
    }
}
