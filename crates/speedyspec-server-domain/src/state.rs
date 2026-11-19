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
    /// The epoch of the leader that wrote this state. Followers ignore a state older than one they have seen.
    pub epoch: u64,
    /// A set the leader has staged for every pod to prepare, before it becomes active.
    pub staged: Option<StagedSet>,
    /// A server upgrade rolling across the pods.
    pub rollout: Option<Rollout>,
    /// Apps held in maintenance by an operator or a failed migration.
    pub maintenance: BTreeMap<AppId, MaintenanceRecord>,
}

/// Why an app is held in maintenance, as recorded in the state file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaintenanceRecord {
    /// An operator's `maintenance on`.
    pub operator: bool,
    /// A migration that failed, and why: maintenance stays on until it is retried or cleared.
    pub migration_failed: Option<String>,
    /// The release whose migration failed (what `release retry-migration` switches to).
    pub migration_version: Option<semver::Version>,
    /// A migration is running (the leader records it so every replica shows the maintenance page).
    pub migrating: bool,
}

impl MaintenanceRecord {
    pub fn is_empty(&self) -> bool {
        !self.operator && self.migration_failed.is_none() && !self.migrating
    }
}

/// Releases staged for every pod to prepare (the first step of a cluster switch).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedSet {
    /// The revision the set was staged at; prepare reports name it.
    pub revision: u64,
    pub targets: Vec<(AppId, Version)>,
    /// When it was staged, in milliseconds since the Unix epoch (for display only).
    #[serde(default)]
    pub since_ms: u64,
}

/// A server upgrade going pod by pod.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rollout {
    /// Which rollout this is (the state revision it began at), so failures are counted only within it.
    #[serde(default)]
    pub id: u64,
    pub target: Version,
    pub previous: Version,
    /// The apps switched with the server, if any.
    #[serde(default)]
    pub group: Option<PendingGroup>,
    /// The pods in upgrade order.
    #[serde(default)]
    pub order: Vec<String>,
    /// The pods that have finished their turn.
    #[serde(default)]
    pub done: Vec<String>,
    /// The pod upgrading now.
    #[serde(default)]
    pub turn: Option<String>,
    /// Set once a trial failed: the pods in `done` go back to `previous`.
    #[serde(default)]
    pub reverting: bool,
    /// The pod whose trial failed, and why.
    #[serde(default)]
    pub failure: Option<(String, String)>,
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
            r#"{"revision":1,"shards":7,"apps":{"admin":{"active":"1.0.0","trial":true}}}"#,
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

    #[test]
    fn cluster_fields_default_when_absent() {
        let state: ReleaseState = serde_json::from_str(
            r#"{"revision":9,"apps":{},"server":{"confirmed":"0.1.0"},"pending_group":null}"#,
        )
        .unwrap();
        assert_eq!(state.epoch, 0);
        assert_eq!(state.staged, None);
        assert_eq!(state.rollout, None);
    }

    #[test]
    fn staged_set_round_trips() {
        let mut state = ReleaseState::default();
        state.epoch = 3;
        state.staged = Some(StagedSet {
            revision: 12,
            targets: vec![(AppId::parse("admin").unwrap(), Version::new(2, 0, 0))],
            since_ms: 1_700_000_000_000,
        });
        state.rollout = Some(Rollout {
            id: 0,
            target: Version::new(0, 2, 0),
            previous: Version::new(0, 1, 0),
            group: None,
            order: vec!["a".into(), "b".into()],
            done: vec!["a".into()],
            turn: Some("b".into()),
            reverting: false,
            failure: None,
        });
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<ReleaseState>(&json).unwrap(), state);
    }
}
