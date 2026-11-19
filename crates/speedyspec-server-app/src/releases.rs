//! The release lifecycle: check, install, switch, roll back, pin, prune and recover at start-up. Every operation runs
//! one at a time, under one lock that also guards the release state.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use semver::Version;
use speedyspec_server_domain::{
    AppId, AppKind, AppRuntime, BinaryInspector, IndexEntry, MemberReport, PendingGroup, Prepared,
    ReleaseManifest, ReleaseSource, ReleaseState, ReleaseStore, SignatureVerifier, SourceError,
    StagedSet, StateError, StateStore, StoreError,
};

use crate::compat::{Incompatibility, check_set};
use crate::download::sha256_file;

/// How an app's artifacts are checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signing {
    /// Keys are configured: every artifact must carry a valid signature.
    Verify,
    /// No keys, `allow_unsigned = true`: signatures are skipped, with a warning.
    AllowUnsigned,
    /// No keys and unsigned releases not allowed: nothing can be installed.
    NoKeys,
}

/// What the lifecycle needs to know about one app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPolicy {
    pub kind: AppKind,
    /// The release index URL.
    pub source: Option<String>,
    /// Kept releases, at least 1.
    pub keep: u32,
    pub prerelease: bool,
    pub signing: Signing,
}

/// Why a release operation was refused or failed. Every message names the app and what to do.
#[derive(Debug, thiserror::Error)]
pub enum ReleaseError {
    #[error("{0} is not a configured app")]
    UnknownApp(AppId),
    #[error("app.{0}.source is not set, so there is nowhere to get releases from")]
    NoSource(AppId),
    #[error("switching the server is done by the keeper; use `release switch server@<version>`")]
    ServerSwitchViaKeeper,
    #[error(
        "switch runtime {runtime} on its own: {apps} run on it, and a runtime switch moves them by itself"
    )]
    RuntimeWithItsApps { runtime: AppId, apps: String },
    #[error("{app} {version}: migration failed: {reason}")]
    Migration {
        app: AppId,
        version: Version,
        reason: String,
    },
    #[error("{0}: a migration is pending; retry or clear it first")]
    MigrationPending(AppId),
    #[error("{0}: no failed migration to retry")]
    NoFailedMigration(AppId),
    #[error("an upgrade to server {0} is on trial; wait for it to be confirmed or to fail")]
    UpgradeInProgress(Version),
    #[error("{0}")]
    Inspect(String),
    #[error("{0} {1} has no artifact for target {2}")]
    NoArtifactForTarget(AppId, Version, String),
    #[error("{0}: nothing newer to install")]
    NothingNewer(AppId),
    #[error("{0} {1} is not in the release index")]
    NotInIndex(AppId, Version),
    #[error("{0} {1} has no artifact for target `any`")]
    NoArtifact(AppId, Version),
    #[error("{app} {version}: checksum mismatch, expected {expected}, got {actual}")]
    Checksum {
        app: AppId,
        version: Version,
        expected: String,
        actual: String,
    },
    #[error("{0}")]
    Signature(String),
    #[error(
        "{0}: no signing keys; set app.{0}.keys, or allow_unsigned = true to install unsigned releases"
    )]
    Unsigned(AppId),
    #[error("{0}")]
    Source(#[from] SourceError),
    #[error("{0}")]
    Store(#[from] StoreError),
    #[error("{0}")]
    State(#[from] StateError),
    #[error("{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Incompatible(Vec<Incompatibility>),
    #[error("{0} {1} is not kept; install it first")]
    NotKept(AppId, Version),
    #[error("{0}: no previous release is kept to roll back to")]
    NoPrevious(AppId),
    #[error("{app} {version} failed to prepare: {reason}")]
    Prepare {
        app: AppId,
        version: Version,
        reason: String,
    },
    #[error("app.{0}.keep must be at least 1")]
    KeepTooSmall(AppId),
    #[error("the cluster's member files cannot be read right now; try again shortly")]
    MembersUnreadable,
    #[error("{}", match .leader {
        Some(leader) => format!("this pod is not the leader; run release commands on {leader}"),
        None => "there is no leader right now; try again shortly".to_string(),
    })]
    NotLeader { leader: Option<String> },
}

/// Runs every release operation. Holds the release state in memory, saved after every change.
pub struct ReleaseManager {
    pub(crate) apps: BTreeMap<AppId, AppPolicy>,
    pub(crate) server: ReleaseManifest,
    pub(crate) source: Arc<dyn ReleaseSource>,
    pub(crate) verifier: Arc<dyn SignatureVerifier>,
    pub(crate) store: Arc<dyn ReleaseStore>,
    pub(crate) state_store: Arc<dyn StateStore>,
    pub(crate) runtimes: HashMap<AppKind, Arc<dyn AppRuntime>>,
    pub(crate) inspector: Arc<dyn BinaryInspector>,
    /// Reads runtime binaries' manifests; the server's inspector when none is set.
    pub(crate) runtime_inspector: Option<Arc<dyn BinaryInspector>>,
    /// Each php app's runtime app, for the runtime requirement in the set check.
    pub(crate) php_runtimes: BTreeMap<AppId, AppId>,
    /// Runs a release's migration on its prepared (not yet live) release.
    pub(crate) migrator: Option<Arc<dyn Migrator>>,
    /// Turns this replica's upgrade maintenance on and off at once (the state file tells the others).
    pub(crate) maintenance_sink: Option<Arc<dyn MaintenanceSink>>,
    /// The apps a release's `"site"` maintenance scope means.
    pub(crate) site_apps: Vec<AppId>,
    /// How long a migration may run.
    pub(crate) migrate_timeout: std::time::Duration,
    /// The Rust target triple whose server artifacts this build installs.
    pub(crate) target: String,
    pub(crate) state: Mutex<ReleaseState>,
    /// A copy of the last saved state, read by `status` and `check` without waiting for operations.
    pub(crate) published: Mutex<ReleaseState>,
    pub(crate) downloads: Mutex<HashMap<AppId, Arc<Mutex<()>>>>,
    pub(crate) last_boot: Mutex<Option<BootReport>>,
    pub(crate) cluster: Arc<crate::Cluster>,
    /// What this pod serves and holds prepared, and the report it last published.
    pub(crate) node: Mutex<Node>,
    /// The Lease epoch this pod last took over at as leader; `None` while it follows.
    pub(crate) took_over: Mutex<Option<u64>>,
}

/// This pod's own view: what it serves, what it holds prepared for a staged set, and its member report.
#[derive(Default)]
pub(crate) struct Node {
    pub(crate) serving: BTreeMap<AppId, Version>,
    pub(crate) held: Option<(u64, Vec<(AppKind, Prepared)>)>,
    pub(crate) report: MemberReport,
}

/// Runs a release's migration on its prepared, not yet live, release (the server sends it as an internal request).
pub trait Migrator: Send + Sync {
    fn migrate(
        &self,
        prepared: &Prepared,
        path: &str,
        timeout: std::time::Duration,
    ) -> Result<(), String>;
}

/// Where this replica's upgrade maintenance is turned on and off at once.
pub trait MaintenanceSink: Send + Sync {
    fn upgrade(&self, apps: &[AppId], on: bool);
}

/// What one follower step did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowOutcome {
    /// Whether `state.json` changed since the last step.
    pub changed: bool,
    /// Whether this pod serves the active set (the readiness probe's answer).
    pub ready: bool,
}

impl ReleaseManager {
    pub fn new(
        apps: BTreeMap<AppId, AppPolicy>,
        server: ReleaseManifest,
        source: Arc<dyn ReleaseSource>,
        verifier: Arc<dyn SignatureVerifier>,
        store: Arc<dyn ReleaseStore>,
        state_store: Arc<dyn StateStore>,
        runtimes: HashMap<AppKind, Arc<dyn AppRuntime>>,
        inspector: Arc<dyn BinaryInspector>,
        target: String,
        cluster: Arc<crate::Cluster>,
    ) -> Result<ReleaseManager, ReleaseError> {
        if let Some((app, _)) = apps.iter().find(|(_, policy)| policy.keep == 0) {
            return Err(ReleaseError::KeepTooSmall(app.clone()));
        }
        let state = state_store.load()?;
        cluster.observe_epoch(state.epoch);

        Ok(ReleaseManager {
            apps,
            server,
            source,
            verifier,
            store,
            state_store,
            runtimes,
            inspector,
            runtime_inspector: None,
            php_runtimes: BTreeMap::new(),
            migrator: None,
            maintenance_sink: None,
            site_apps: Vec::new(),
            migrate_timeout: std::time::Duration::from_secs(30 * 60),
            target,
            published: Mutex::new(state.clone()),
            downloads: Mutex::new(HashMap::new()),
            state: Mutex::new(state),
            last_boot: Mutex::new(None),
            cluster,
            node: Mutex::new(Node::default()),
            took_over: Mutex::new(None),
        })
    }

    /// Reads runtime binaries' manifests with `inspector` (they run as `<binary> manifest`, as the server does, but
    /// print a different manifest).
    pub fn with_runtime_inspector(mut self, inspector: Arc<dyn BinaryInspector>) -> ReleaseManager {
        self.runtime_inspector = Some(inspector);
        self
    }

    /// Runs releases' migrations.
    pub fn with_migrator(mut self, migrator: Arc<dyn Migrator>) -> ReleaseManager {
        self.migrator = Some(migrator);
        self
    }

    /// Where upgrade maintenance is turned on and off at once, and the apps a `"site"` scope means.
    pub fn with_maintenance(
        mut self,
        sink: Arc<dyn MaintenanceSink>,
        site_apps: Vec<AppId>,
    ) -> ReleaseManager {
        self.maintenance_sink = Some(sink);
        self.site_apps = site_apps;
        self
    }

    /// How long a migration may run (`switch.migrate_timeout`).
    pub fn with_migrate_timeout(mut self, timeout: std::time::Duration) -> ReleaseManager {
        self.migrate_timeout = timeout;
        self
    }

    /// Each php app's runtime app: a php release's `php.runtime` is checked against it.
    pub fn with_php_runtimes(mut self, runtimes: BTreeMap<AppId, AppId>) -> ReleaseManager {
        self.php_runtimes = runtimes;
        self
    }

    fn binary_inspector(&self, kind: AppKind) -> &Arc<dyn BinaryInspector> {
        match (kind, &self.runtime_inspector) {
            (AppKind::Runtime, Some(inspector)) => inspector,
            _ => &self.inspector,
        }
    }

    /// Takes the operation lock. A panic in an earlier operation does not poison it for later ones.
    pub(crate) fn lock(&self) -> MutexGuard<'_, ReleaseState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Refuses up front, before any download, when this pod does not lead.
    pub(crate) fn leading(&self) -> Result<(), ReleaseError> {
        let view = self.cluster.view();
        if view.is_leader {
            Ok(())
        } else {
            Err(ReleaseError::NotLeader {
                leader: view.leader,
            })
        }
    }

    /// This pod's cluster.
    pub fn cluster(&self) -> &Arc<crate::Cluster> {
        &self.cluster
    }

    /// Reloads `state.json` written by the leader. A state stamped with an epoch older than the Lease's current
    /// epoch, or than one already seen, is ignored: it is a deposed leader's late write, or a state the new leader has
    /// not re-stamped yet (it does so on taking over). Returns whether the state in memory changed.
    pub fn refresh(&self) -> Result<bool, ReleaseError> {
        self.cluster.observe_epoch(self.cluster.view().epoch);
        self.reload()
    }

    /// Reloads `state.json` unless it is older than an epoch already seen.
    fn reload(&self) -> Result<bool, ReleaseError> {
        // Read under the lock, so a write this pod commits meanwhile is never replaced by the file read before it.
        let mut state = self.lock();
        let loaded = self.state_store.load()?;
        if !self.cluster.observe_epoch(loaded.epoch) {
            tracing::warn!(
                event = "stale_epoch_ignored",
                epoch = loaded.epoch,
                revision = loaded.revision
            );
            return Ok(false);
        }
        if loaded.epoch == state.epoch && loaded.revision < state.revision {
            // Within one epoch revisions only grow: the file is older than what this pod already has.
            return Ok(false);
        }
        if *state == loaded {
            return Ok(false);
        }
        *state = loaded;
        *self.published.lock().unwrap_or_else(|p| p.into_inner()) = state.clone();
        Ok(true)
    }

    pub(crate) fn policy(&self, app: &AppId) -> Result<&AppPolicy, ReleaseError> {
        self.apps
            .get(app)
            .ok_or_else(|| ReleaseError::UnknownApp(app.clone()))
    }

    fn index_url(&self, app: &AppId) -> Result<&str, ReleaseError> {
        self.policy(app)?
            .source
            .as_deref()
            .ok_or_else(|| ReleaseError::NoSource(app.clone()))
    }

    /// The newest version worth installing: newer than the active one, allowed by `prerelease`, not failed. `None`
    /// when pinned or up to date.
    pub fn check(&self, app: &AppId) -> Result<Option<Version>, ReleaseError> {
        let candidate = self.candidate(&self.snapshot(), app)?;
        Ok(candidate.map(|entry| entry.version))
    }

    fn candidate(
        &self,
        state: &ReleaseState,
        app: &AppId,
    ) -> Result<Option<IndexEntry>, ReleaseError> {
        let policy = self.policy(app)?;
        let current = state.apps.get(app).cloned().unwrap_or_default();
        if current.pinned.is_some() {
            return Ok(None);
        }
        let url = self.index_url(app)?;
        let index = self.source.index(url)?;
        if index.app != *app {
            return Err(SourceError {
                url: url.to_string(),
                message: format!("the index is for {}, not {app}", index.app),
            }
            .into());
        }
        Ok(index
            .releases
            .into_iter()
            .filter(|entry| policy.prerelease || entry.version.pre.is_empty())
            .filter(|entry| {
                current
                    .active
                    .as_ref()
                    .is_none_or(|active| entry.version > *active)
            })
            .filter(|entry| !current.failed.contains_key(&entry.version))
            .max_by(|a, b| a.version.cmp(&b.version)))
    }

    /// Downloads, verifies, unpacks and keeps a release, without switching to it. With `None`, installs `check`'s
    /// candidate. A version already kept is not downloaded again. The download runs without the state lock, so
    /// `status`, `pin` and `rollback` never wait on it.
    pub fn install(&self, app: &AppId, version: Option<Version>) -> Result<Version, ReleaseError> {
        self.leading()?;
        self.installable(app)?;
        let guard = self.download_guard(app);
        let _downloading = guard
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = match version {
            Some(version) => {
                if self
                    .snapshot()
                    .apps
                    .get(app)
                    .is_some_and(|current| current.kept.contains(&version))
                {
                    return Ok(version);
                }
                self.index_entry(app, &version)?
            }
            None => self
                .candidate(&self.snapshot(), app)?
                .ok_or_else(|| ReleaseError::NothingNewer(app.clone()))?,
        };
        let version = self.fetch(app, &entry)?;
        let mut state = self.lock();
        self.unpack_and_keep(&mut state, app, &version)?;
        Ok(version)
    }

    /// One auto-update step: install the candidate and switch to it. The candidate is checked again under the lock
    /// just before the switch, so a pin or rollback issued during the download wins. Returns the version switched to.
    pub fn update(&self, app: &AppId) -> Result<Option<Version>, ReleaseError> {
        self.leading()?;
        if self.installable(app)?.kind == AppKind::Server {
            return Err(ReleaseError::ServerSwitchViaKeeper);
        }
        let guard = self.download_guard(app);
        let _downloading = guard
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = self.candidate(&self.snapshot(), app)? else {
            return Ok(None);
        };
        let version = self.fetch(app, &entry)?;
        let mut state = self.lock();
        self.unpack_and_keep(&mut state, app, &version)?;
        let current = state.apps.get(app).cloned().unwrap_or_default();
        let still_wanted = current.pinned.is_none()
            && !current.failed.contains_key(&version)
            && current
                .active
                .as_ref()
                .is_none_or(|active| version > *active);
        if !still_wanted {
            tracing::info!(event = "auto_update_skipped", app = %app, version = %version, "the app changed during the download");
            return Ok(None);
        }
        self.switch_locked(&mut state, &[(app.clone(), version.clone())])?;
        Ok(Some(version))
    }

    fn index_entry(&self, app: &AppId, version: &Version) -> Result<IndexEntry, ReleaseError> {
        let url = self.index_url(app)?;
        let index = self.source.index(url)?;
        if index.app != *app {
            return Err(SourceError {
                url: url.to_string(),
                message: format!("the index is for {}, not {app}", index.app),
            }
            .into());
        }
        index
            .entry(version)
            .cloned()
            .ok_or_else(|| ReleaseError::NotInIndex(app.clone(), version.clone()))
    }

    /// The app's policy, if releases can be installed for it at all.
    fn installable(&self, app: &AppId) -> Result<&AppPolicy, ReleaseError> {
        let policy = self.policy(app)?;
        if policy.signing == Signing::NoKeys {
            return Err(ReleaseError::Unsigned(app.clone()));
        }
        Ok(policy)
    }

    /// The per-app guard that keeps two downloads of one app apart.
    fn download_guard(&self, app: &AppId) -> Arc<Mutex<()>> {
        let mut guards = self
            .downloads
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guards.entry(app.clone()).or_default().clone()
    }

    /// Downloads and verifies an entry's artifact into its partial directory, without the state lock. A release that
    /// fails verification is recorded as failed, so it is not offered again; a network failure is not, so the next
    /// attempt resumes.
    fn fetch(&self, app: &AppId, entry: &IndexEntry) -> Result<Version, ReleaseError> {
        let policy = self.installable(app)?.clone();
        let version = entry.version.clone();
        if self
            .snapshot()
            .apps
            .get(app)
            .is_some_and(|current| current.kept.contains(&version))
        {
            return Ok(version);
        }
        let target = if policy.kind.is_binary() {
            self.target.as_str()
        } else {
            "any"
        };
        let Some(artifact) = entry.artifact_for(target) else {
            let err = if policy.kind.is_binary() {
                ReleaseError::NoArtifactForTarget(app.clone(), version.clone(), target.to_string())
            } else {
                ReleaseError::NoArtifact(app.clone(), version.clone())
            };
            return Err(self.reject(app, &version, err));
        };

        let partial = self.store.partial_dir(app, &version);
        std::fs::create_dir_all(&partial).map_err(|err| store_error(&partial, err))?;
        let archive = partial.join("artifact");
        self.source.download(&artifact.url, &archive)?;
        let actual = sha256_file(&archive).map_err(|err| store_error(&archive, err))?;
        if actual != artifact.sha256 {
            let err = ReleaseError::Checksum {
                app: app.clone(),
                version: version.clone(),
                expected: artifact.sha256.clone(),
                actual,
            };
            return Err(self.reject(app, &version, err));
        }
        match policy.signing {
            Signing::Verify => {
                let Some(signature_url) = &artifact.minisig else {
                    let err = ReleaseError::Signature(format!(
                        "{app} {version}: the index lists no signature"
                    ));
                    return Err(self.reject(app, &version, err));
                };
                let signature = partial.join("artifact.minisig");
                let _ = std::fs::remove_file(&signature);
                self.source.download(signature_url, &signature)?;
                if let Err(err) = self.verifier.verify(app, &archive, &signature) {
                    return Err(self.reject(app, &version, ReleaseError::Signature(err.0)));
                }
            }
            Signing::AllowUnsigned => {
                tracing::warn!(event = "unsigned_install", app = %app, version = %version);
            }
            Signing::NoKeys => unreachable!("refused above"),
        }

        Ok(version)
    }

    /// Unpacks a fetched release and keeps it. Refused archives and kind mismatches are recorded as failed.
    fn unpack_and_keep(
        &self,
        state: &mut ReleaseState,
        app: &AppId,
        version: &Version,
    ) -> Result<(), ReleaseError> {
        if state
            .apps
            .get(app)
            .is_some_and(|current| current.kept.contains(version))
        {
            return Ok(());
        }
        if self.policy(app)?.kind.is_binary() {
            return self.keep_binary(state, app, version);
        }
        let policy = self.policy(app)?.clone();
        let partial = self.store.partial_dir(app, version);
        let archive = partial.join("artifact");
        let manifest = match self.store.unpack(app, version, &archive) {
            Ok(manifest) => manifest,
            Err(err) => return Err(self.reject_locked(state, app, version, err.into())),
        };
        if manifest.kind != policy.kind {
            self.store.discard_partial(app, version);
            let err = store_error(
                &partial,
                format!(
                    "release kind is {}, but app.{app}.kind is {}",
                    manifest.kind.as_str(),
                    policy.kind.as_str()
                ),
            );
            return Err(self.reject_locked(state, app, version, err.into()));
        }
        self.store.commit(app, version)?;

        let mut next = state.clone();
        let current = next.app_mut(app);
        current.kept.retain(|kept| kept != version);
        current.kept.push(version.clone());
        current.failed.remove(version);
        self.commit_state(state, next)?;
        tracing::info!(event = "installed", app = %app, version = %version);
        Ok(())
    }

    /// Keeps a verified binary (the server's or a runtime's): moves it into place, then reads its manifest by running
    /// it. A runtime's manifest is kept beside it, with this app's id, so switches can read it.
    fn keep_binary(
        &self,
        state: &mut ReleaseState,
        app: &AppId,
        version: &Version,
    ) -> Result<(), ReleaseError> {
        let kind = self.policy(app)?.kind;
        let name = kind.binary_name().expect("a binary kind");
        let archive = self.store.partial_dir(app, version).join("artifact");
        let path = self.store.commit_binary(app, version, &archive, name)?;
        let checked = self
            .binary_inspector(kind)
            .manifest(&path)
            .and_then(|manifest| {
                if manifest.kind != kind || manifest.version != *version {
                    Err(format!(
                        "{} reports kind {} version {}, expected a {} binary of version {version}",
                        path.display(),
                        manifest.kind.as_str(),
                        manifest.version,
                        kind.as_str()
                    ))
                } else {
                    Ok(manifest)
                }
            });
        let checked = match checked {
            Ok(mut manifest) if kind == AppKind::Runtime => {
                manifest.app = app.clone();
                self.store
                    .write_manifest(app, version, &manifest)
                    .map_err(|err| err.to_string())
            }
            Ok(_) => Ok(()),
            Err(reason) => Err(reason),
        };
        if let Err(reason) = checked {
            let _ = self.store.remove(app, version);
            return Err(self.reject_locked(state, app, version, ReleaseError::Inspect(reason)));
        }
        let mut next = state.clone();
        let current = next.app_mut(app);
        current.kept.retain(|kept| kept != version);
        current.kept.push(version.clone());
        current.failed.remove(version);
        self.commit_state(state, next)?;
        tracing::info!(event = "installed", app = %app, version = %version);
        Ok(())
    }

    /// Discards a release's partial download, records it as failed with the error, and returns the error.
    fn reject(&self, app: &AppId, version: &Version, err: ReleaseError) -> ReleaseError {
        let mut state = self.lock();
        self.reject_locked(&mut state, app, version, err)
    }

    fn reject_locked(
        &self,
        state: &mut ReleaseState,
        app: &AppId,
        version: &Version,
        err: ReleaseError,
    ) -> ReleaseError {
        self.store.discard_partial(app, version);
        let mut next = state.clone();
        next.app_mut(app)
            .failed
            .insert(version.clone(), err.to_string());
        if let Err(save) = self.commit_state(state, next) {
            tracing::error!(event = "failure_not_recorded", app = %app, version = %version, error = %save);
        }
        tracing::warn!(event = "release_refused", app = %app, version = %version, error = %err);
        err
    }

    /// The last saved state, without waiting for a running operation.
    pub(crate) fn snapshot(&self) -> ReleaseState {
        self.published
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl ReleaseManager {
    /// Saves `next` with a new revision; only once that succeeds does it become the state in memory.
    pub(crate) fn commit_state(
        &self,
        state: &mut ReleaseState,
        mut next: ReleaseState,
    ) -> Result<(), ReleaseError> {
        let view = self.cluster.view();
        if !view.is_leader {
            return Err(ReleaseError::NotLeader {
                leader: view.leader,
            });
        }
        next.epoch = self.cluster.stamp(view.epoch);
        self.cluster.observe_epoch(next.epoch);
        next.revision = state.revision + 1;
        self.state_store.save(&next)?;
        *state = next;
        *self
            .published
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = state.clone();
        Ok(())
    }

    fn runtime(&self, kind: AppKind) -> Option<&Arc<dyn AppRuntime>> {
        self.runtimes.get(&kind)
    }

    /// The manifest of every active release, the server's included.
    pub fn active_manifests(&self) -> BTreeMap<AppId, ReleaseManifest> {
        self.manifests_of(&self.snapshot())
    }

    pub(crate) fn manifests_of(&self, state: &ReleaseState) -> BTreeMap<AppId, ReleaseManifest> {
        let mut manifests = BTreeMap::new();
        manifests.insert(self.server.app.clone(), self.server.clone());
        for (app, current) in &state.apps {
            if let Some(active) = &current.active
                && let Ok(manifest) = self.store.manifest(app, active)
            {
                manifests.insert(app.clone(), manifest);
            }
        }
        manifests
    }

    /// A copy of the release state.
    pub fn status(&self) -> ReleaseState {
        self.snapshot()
    }

    /// Switches one or several apps to kept versions, as one step: every target is checked for compatibility
    /// against the resulting set and prepared first; if any fails, none switches.
    pub fn switch(&self, targets: &[(AppId, Version)]) -> Result<(), ReleaseError> {
        self.leading()?;
        let mut state = self.lock();
        self.switch_locked(&mut state, targets)
    }

    pub(crate) fn switch_locked(
        &self,
        state: &mut ReleaseState,
        targets: &[(AppId, Version)],
    ) -> Result<(), ReleaseError> {
        self.check_runtime_alone(targets)?;
        self.check_no_pending_migration(state, targets)?;
        if !self.cluster.is_single() {
            return self.switch_staged(state, targets);
        }
        for (app, version) in targets {
            let policy = self.policy(app)?;
            if policy.kind == AppKind::Server {
                return Err(ReleaseError::ServerSwitchViaKeeper);
            }
            if !state
                .apps
                .get(app)
                .is_some_and(|current| current.kept.contains(version))
            {
                return Err(ReleaseError::NotKept(app.clone(), version.clone()));
            }
        }
        let targets: Vec<&(AppId, Version)> = targets
            .iter()
            .filter(|(app, version)| {
                state.apps.get(app).and_then(|c| c.active.as_ref()) != Some(version)
            })
            .collect();
        if targets.is_empty() {
            return Ok(());
        }

        let mut manifests = self.manifests_of(state);
        let mut incoming = Vec::with_capacity(targets.len());
        for (app, version) in &targets {
            let manifest = self.store.manifest(app, version)?;
            manifests.insert(app.clone(), manifest.clone());
            incoming.push((app.clone(), version.clone(), manifest));
        }
        check_set(&manifests, &self.php_runtimes).map_err(ReleaseError::Incompatible)?;

        let mut prepared = Vec::with_capacity(incoming.len());
        let mut failures = Vec::new();
        for (app, version, manifest) in &incoming {
            let dir = self.store.release_dir(app, version);
            let result = match self.runtime(manifest.kind) {
                Some(runtime) => runtime.prepare(app, manifest, &dir),
                None => Err(format!("no runtime for kind {}", manifest.kind.as_str())),
            };
            match result {
                Ok(ready) => prepared.push((manifest.kind, ready)),
                Err(reason) => failures.push((app.clone(), version.clone(), reason)),
            }
        }
        if let Some((app, version, reason)) = failures.first().cloned() {
            let mut next = state.clone();
            for (app, version, reason) in &failures {
                next.app_mut(app)
                    .failed
                    .insert(version.clone(), reason.clone());
            }
            self.commit_state(state, next)?;
            return Err(ReleaseError::Prepare {
                app,
                version,
                reason,
            });
        }

        let old: Vec<(AppId, Option<Version>)> = targets
            .iter()
            .map(|(app, _)| {
                (
                    app.clone(),
                    state.apps.get(app).and_then(|c| c.active.clone()),
                )
            })
            .collect();
        let scopes = self.migrate_all(state, &prepared, false)?;
        for (kind, ready) in prepared {
            self.go_live(kind, ready);
        }
        let mut next = state.clone();
        for (app, version) in &targets {
            let current = next.app_mut(app);
            current.previous = current.active.replace(version.clone());
            current.failed.remove(version);
        }
        if let Err(err) = self.commit_state(state, next) {
            self.reactivate(&old);
            return Err(err);
        }
        self.finish_migrations(state, &scopes)?;
        for (app, previous) in &old {
            if let Some(previous) = previous {
                let kind = self.policy(app)?.kind;
                if let Some(runtime) = self.runtime(kind) {
                    runtime.retire(app, previous);
                }
            }
            tracing::info!(event = "switched", app = %app, version = %state.apps[app].active.as_ref().expect("set"));
        }
        let apps: Vec<AppId> = targets.iter().map(|(app, _)| app.clone()).collect();
        self.prune(state, &apps)?;

        Ok(())
    }

    /// Whether any target's release migrates.
    fn migrates(&self, targets: &[(AppId, Version)]) -> bool {
        targets.iter().any(|(app, version)| {
            self.store
                .manifest(app, version)
                .ok()
                .and_then(|m| m.php_section())
                .is_some_and(|php| php.switch_.is_some())
        })
    }

    /// The apps a migrating release's maintenance scope covers.
    fn scope(&self, app: &AppId, scope: speedyspec_server_domain::MaintenanceScope) -> Vec<AppId> {
        match scope {
            speedyspec_server_domain::MaintenanceScope::SelfOnly => vec![app.clone()],
            speedyspec_server_domain::MaintenanceScope::Site if !self.site_apps.is_empty() => {
                self.site_apps.clone()
            }
            speedyspec_server_domain::MaintenanceScope::Site => self
                .apps
                .iter()
                .filter(|(_, p)| !matches!(p.kind, AppKind::Server | AppKind::Runtime))
                .map(|(id, _)| id.clone())
                .collect(),
        }
    }

    /// Runs the migration of every prepared release that has one, in turn: maintenance on (here at once, and in the
    /// state file for every replica), then the migration on the prepared release. A failed migration is recorded
    /// (the release failed, maintenance kept on) and fails the switch. Returns each migration's scope, for
    /// [`Self::finish_migrations`] once the switch is done.
    fn migrate_all(
        &self,
        state: &mut ReleaseState,
        prepared: &[(AppKind, Prepared)],
        cluster: bool,
    ) -> Result<Vec<Vec<AppId>>, ReleaseError> {
        let mut scopes = Vec::new();
        for (kind, ready) in prepared {
            if *kind != AppKind::Php {
                continue;
            }
            let Some(switch) = self
                .store
                .manifest(&ready.app, &ready.version)
                .ok()
                .and_then(|m| m.php_section())
                .and_then(|php| php.switch_)
            else {
                continue;
            };
            let scope = self.scope(&ready.app, switch.maintenance);
            let mut next = state.clone();
            for app in &scope {
                next.maintenance.entry(app.clone()).or_default().migrating = true;
            }
            self.commit_state(state, next)?;
            if let Some(sink) = &self.maintenance_sink {
                sink.upgrade(&scope, true);
            }
            if cluster {
                // The followers show the maintenance page from their next poll of the state.
                std::thread::sleep(self.cluster.poll_interval * 2);
            }
            tracing::info!(event = "migrating", app = %ready.app, version = %ready.version, path = %switch.migrate);
            let outcome = match &self.migrator {
                Some(migrator) => migrator.migrate(ready, &switch.migrate, self.migrate_timeout),
                None => Err("this server cannot run migrations".to_string()),
            };
            if let Err(reason) = outcome {
                let mut next = state.clone();
                for app in &scope {
                    let record = next.maintenance.entry(app.clone()).or_default();
                    record.migrating = false;
                    record.migration_failed = Some(reason.clone());
                    record.migration_version = Some(ready.version.clone());
                }
                next.app_mut(&ready.app)
                    .failed
                    .insert(ready.version.clone(), format!("migration failed: {reason}"));
                self.commit_state(state, next)?;
                tracing::error!(event = "migration_failed", app = %ready.app, version = %ready.version, reason = %reason);
                return Err(ReleaseError::Migration {
                    app: ready.app.clone(),
                    version: ready.version.clone(),
                    reason,
                });
            }
            scopes.push(scope);
        }
        Ok(scopes)
    }

    /// Ends the upgrade maintenance of migrations whose switch is done.
    fn finish_migrations(
        &self,
        state: &mut ReleaseState,
        scopes: &[Vec<AppId>],
    ) -> Result<(), ReleaseError> {
        if scopes.is_empty() {
            return Ok(());
        }
        let mut next = state.clone();
        for app in scopes.iter().flatten() {
            if let Some(record) = next.maintenance.get_mut(app) {
                record.migrating = false;
            }
        }
        next.maintenance.retain(|_, record| !record.is_empty());
        self.commit_state(state, next)?;
        if let Some(sink) = &self.maintenance_sink {
            for scope in scopes {
                sink.upgrade(scope, false);
            }
        }
        Ok(())
    }

    /// Refuses to switch a runtime while a migration runs or has failed.
    fn check_no_pending_migration(
        &self,
        state: &ReleaseState,
        targets: &[(AppId, Version)],
    ) -> Result<(), ReleaseError> {
        let runtime_switch = targets
            .iter()
            .any(|(app, _)| self.policy(app).is_ok_and(|p| p.kind == AppKind::Runtime));
        if !runtime_switch {
            return Ok(());
        }
        match state
            .maintenance
            .iter()
            .find(|(_, record)| record.migrating || record.migration_failed.is_some())
        {
            Some((app, _)) => Err(ReleaseError::MigrationPending(app.clone())),
            None => Ok(()),
        }
    }

    /// Switches `app` to the release whose migration failed, again (its failure cleared first).
    pub fn retry_migration(&self, app: &AppId) -> Result<(), ReleaseError> {
        self.leading()?;
        let mut state = self.lock();
        let version = state
            .maintenance
            .get(app)
            .and_then(|record| record.migration_version.clone())
            .ok_or_else(|| ReleaseError::NoFailedMigration(app.clone()))?;
        let mut next = state.clone();
        for record in next.maintenance.values_mut() {
            if record.migration_version.as_ref() == Some(&version) {
                record.migration_failed = None;
                record.migration_version = None;
            }
        }
        next.maintenance.retain(|_, record| !record.is_empty());
        next.app_mut(app).failed.remove(&version);
        self.commit_state(&mut state, next)?;
        self.switch_locked(&mut state, &[(app.clone(), version)])
    }

    /// Refuses a switch that names a runtime app together with php apps on it: the runtime's switch starts those apps
    /// again on the new binary, so their own switch in the same step would race it.
    fn check_runtime_alone(&self, targets: &[(AppId, Version)]) -> Result<(), ReleaseError> {
        for (runtime, _) in targets {
            if self
                .policy(runtime)
                .is_ok_and(|p| p.kind == AppKind::Runtime)
            {
                let apps: Vec<&str> = targets
                    .iter()
                    .filter(|(app, _)| self.php_runtimes.get(app) == Some(runtime))
                    .map(|(app, _)| app.as_str())
                    .collect();
                if !apps.is_empty() {
                    return Err(ReleaseError::RuntimeWithItsApps {
                        runtime: runtime.clone(),
                        apps: apps.join(", "),
                    });
                }
            }
        }
        Ok(())
    }

    /// Puts the old releases back live after a switch whose state could not be saved, so memory matches disk.
    fn reactivate(&self, old: &[(AppId, Option<Version>)]) {
        for (app, version) in old {
            let Some(version) = version else {
                tracing::error!(event = "memory_and_state_differ", app = %app, "no earlier release to put back");
                continue;
            };
            let restored = self.policy(app).ok().and_then(|policy| {
                let runtime = self.runtime(policy.kind)?;
                let manifest = self.store.manifest(app, version).ok()?;
                let ready = runtime
                    .prepare(app, &manifest, &self.store.release_dir(app, version))
                    .ok()?;
                self.served(&ready.app, &ready.version);
                runtime.activate(ready);
                Some(())
            });
            if restored.is_none() {
                tracing::error!(event = "memory_and_state_differ", app = %app, version = %version);
            }
        }
    }

    /// Removes kept releases beyond each app's `keep`, oldest first. A pinned release does not count against `keep`,
    /// and neither it nor the active release is ever removed.
    fn prune(&self, state: &mut ReleaseState, apps: &[AppId]) -> Result<(), ReleaseError> {
        let mut next = state.clone();
        let mut removed = false;
        let Some(held) = self.held_by_live_members() else {
            tracing::warn!(
                event = "prune_skipped",
                "the member files cannot be read, so nothing is pruned"
            );
            return Ok(());
        };
        for app in apps {
            let keep = self.policy(app)?.keep as usize;
            let current = next.app_mut(app);
            let (active, pinned) = (current.active.clone(), current.pinned.clone());
            let counted = |kept: &[Version]| {
                kept.iter()
                    .filter(|version| Some(*version) != pinned.as_ref())
                    .count()
            };
            while counted(&current.kept) > keep {
                let Some(index) = current.kept.iter().position(|version| {
                    Some(version) != active.as_ref()
                        && Some(version) != pinned.as_ref()
                        && !held.contains(&(app.clone(), version.clone()))
                }) else {
                    break;
                };
                let version = current.kept.remove(index);
                current.failed.remove(&version);
                self.store.remove(app, &version)?;
                tracing::info!(event = "pruned", app = %app, version = %version);
                removed = true;
            }
        }
        if removed {
            self.commit_state(state, next)?;
        }
        Ok(())
    }

    /// Switches back to `version`, or to the previous release, and pins it there: auto-update leaves the app alone
    /// until `unpin`. The version rolled back from is also marked failed ("rolled back"), so it is not offered again
    /// after unpinning; an explicit switch clears that.
    pub fn rollback(&self, app: &AppId, version: Option<Version>) -> Result<Version, ReleaseError> {
        self.leading()?;
        let mut state = self.lock();
        let current = state.apps.get(app).cloned().unwrap_or_default();
        let target = match version {
            Some(version) => version,
            None => current
                .previous
                .clone()
                .filter(|previous| current.kept.contains(previous))
                .ok_or_else(|| ReleaseError::NoPrevious(app.clone()))?,
        };
        self.switch_locked(&mut state, &[(app.clone(), target.clone())])?;
        let mut next = state.clone();
        let entry = next.app_mut(app);
        entry.pinned = Some(target.clone());
        if let Some(from) = current.active.filter(|from| *from != target) {
            entry.failed.insert(from, "rolled back".to_string());
        }
        self.commit_state(&mut state, next)?;
        Ok(target)
    }

    /// Stops auto-update for `app` while `version`, which must be kept, is pinned.
    pub fn pin(&self, app: &AppId, version: Version) -> Result<(), ReleaseError> {
        self.leading()?;
        let mut state = self.lock();
        self.policy(app)?;
        if !state
            .apps
            .get(app)
            .is_some_and(|current| current.kept.contains(&version))
        {
            return Err(ReleaseError::NotKept(app.clone(), version));
        }
        let mut next = state.clone();
        next.app_mut(app).pinned = Some(version);
        self.commit_state(&mut state, next)
    }

    /// Turns an operator's maintenance on or off for `apps` (every configured app when empty). A failed migration's
    /// maintenance is not touched: `clear_maintenance` ends that.
    pub fn set_maintenance(&self, apps: &[AppId], on: bool) -> Result<Vec<AppId>, ReleaseError> {
        self.leading()?;
        let apps = self.maintenance_targets(apps)?;
        let mut state = self.lock();
        let mut next = state.clone();
        for app in &apps {
            next.maintenance.entry(app.clone()).or_default().operator = on;
        }
        next.maintenance.retain(|_, record| !record.is_empty());
        self.commit_state(&mut state, next)?;
        Ok(apps)
    }

    /// Ends every recorded maintenance of `apps` (every configured app when empty): the operator's and a failed
    /// migration's.
    pub fn clear_maintenance(&self, apps: &[AppId]) -> Result<Vec<AppId>, ReleaseError> {
        self.leading()?;
        let apps = self.maintenance_targets(apps)?;
        let mut state = self.lock();
        let mut next = state.clone();
        for app in &apps {
            next.maintenance.remove(app);
        }
        self.commit_state(&mut state, next)?;
        Ok(apps)
    }

    fn maintenance_targets(&self, apps: &[AppId]) -> Result<Vec<AppId>, ReleaseError> {
        if apps.is_empty() {
            return Ok(self
                .apps
                .iter()
                .filter(|(_, policy)| !matches!(policy.kind, AppKind::Server | AppKind::Runtime))
                .map(|(app, _)| app.clone())
                .collect());
        }
        for app in apps {
            self.policy(app)?;
        }
        Ok(apps.to_vec())
    }

    pub fn unpin(&self, app: &AppId) -> Result<(), ReleaseError> {
        self.leading()?;
        let mut state = self.lock();
        self.policy(app)?;
        let mut next = state.clone();
        next.app_mut(app).pinned = None;
        self.commit_state(&mut state, next)
    }
}

/// What start-up recovery found and did.
#[derive(Clone, Debug, Default)]
pub struct BootReport {
    /// Partial downloads that were deleted.
    pub cleaned: Vec<PathBuf>,
    /// (app, the version the state named, the version now active) for every app that could not start on its active
    /// release.
    pub fallbacks: Vec<(AppId, Option<Version>, Option<Version>)>,
    /// Apps with no release that loads.
    pub not_installed: Vec<AppId>,
    /// Requirements the active set does not meet. The server serves anyway, so it can be fixed through it.
    pub incompatible: Vec<Incompatibility>,
}

impl ReleaseManager {
    /// Recovers at start-up: deletes partial downloads, drops kept releases that no longer load, activates every
    /// app's active release, and when that fails falls back to the newest kept release older than it (never a newer
    /// one, which nobody switched to). Saves only if something changed.
    pub fn boot(&self) -> BootReport {
        let mut state = self.lock();
        let mut report = BootReport::default();
        if let Some(group) = state.pending_group.clone()
            && group.server.1 != self.server.version
            && let Err(err) = self.revert_locked(&mut state, &group.server.1, "trial failed")
        {
            tracing::error!(event = "trial_revert_failed", error = %err);
        }
        match self.store.clean_partials() {
            Ok(cleaned) => report.cleaned = cleaned,
            Err(err) => tracing::warn!(event = "clean_partials_failed", error = %err),
        }

        let mut next = state.clone();
        // Runtimes go live before the php apps that run their binary.
        let mut order: Vec<(&AppId, &AppPolicy)> = self.apps.iter().collect();
        order.sort_by_key(|(_, policy)| match policy.kind {
            AppKind::Server | AppKind::Static => 0,
            AppKind::Runtime => 1,
            AppKind::Php => 2,
        });
        for (app, policy) in order {
            if policy.kind == AppKind::Server {
                continue;
            }
            let current = next.app_mut(app);
            let loadable: Vec<Version> = current
                .kept
                .iter()
                .filter(|version| self.store.manifest(app, version).is_ok())
                .cloned()
                .collect();
            current.kept.retain(|version| loadable.contains(version));
            let Some(named) = current.active.clone() else {
                report.not_installed.push(app.clone());
                continue;
            };

            let mut candidates: Vec<Version> = Vec::new();
            if loadable.contains(&named) {
                candidates.push(named.clone());
            }
            let mut older: Vec<Version> = loadable
                .iter()
                .filter(|version| **version < named)
                .cloned()
                .collect();
            older.sort_by(|a, b| b.cmp(a));
            candidates.extend(older);

            let mut chosen = None;
            for version in candidates {
                let prepared = self
                    .store
                    .manifest(app, &version)
                    .map_err(|err| err.to_string())
                    .and_then(|manifest| {
                        let runtime = self.runtime(policy.kind).ok_or_else(|| {
                            format!("no runtime for kind {}", policy.kind.as_str())
                        })?;
                        let ready = runtime.prepare(
                            app,
                            &manifest,
                            &self.store.release_dir(app, &version),
                        )?;
                        self.served(&ready.app, &ready.version);
                        runtime.activate(ready);
                        Ok(())
                    });
                match prepared {
                    Ok(()) => {
                        chosen = Some(version);
                        break;
                    }
                    Err(reason) => {
                        current.failed.insert(version.clone(), reason);
                    }
                }
            }
            if chosen.as_ref() != Some(&named) {
                tracing::warn!(event = "active_fallback", app = %app, was = %named, now = ?chosen.as_ref().map(ToString::to_string));
                report
                    .fallbacks
                    .push((app.clone(), Some(named.clone()), chosen.clone()));
                if chosen.is_none() {
                    report.not_installed.push(app.clone());
                }
                current.previous = None;
                current.active = chosen;
            }
        }
        if next != *state
            && let Err(err) = self.commit_state(&mut state, next)
        {
            tracing::error!(event = "boot_state_not_saved", error = %err);
        }

        if let Err(incompatible) = check_set(&self.manifests_of(&state), &self.php_runtimes) {
            for item in &incompatible {
                tracing::error!(event = "incompatible_set", message = %item);
            }
            report.incompatible = incompatible;
        }
        *self
            .last_boot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(report.clone());

        report
    }

    /// The report of the last `boot`, if it ran.
    pub fn last_boot(&self) -> Option<BootReport> {
        self.last_boot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

/// What the keeper needs to start an upgraded generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerUpgrade {
    pub version: Version,
    pub binary: PathBuf,
    /// In a cluster: the pods in the order they will upgrade. The pods upgrade themselves, turn by turn; on a
    /// single host (`None`) the keeper upgrades now.
    pub rollout: Option<Vec<String>>,
}

/// The file name of a kept server binary inside its release directory.
pub const SERVER_BINARY: &str = "speedyspec-server";

impl ReleaseManager {
    /// The manifest of a kept release: a server binary's comes from running it.
    fn manifest_for(
        &self,
        app: &AppId,
        version: &Version,
    ) -> Result<ReleaseManifest, ReleaseError> {
        if self.policy(app)?.kind == AppKind::Server {
            let binary = self.store.release_dir(app, version).join(SERVER_BINARY);
            return self
                .inspector
                .manifest(&binary)
                .map_err(ReleaseError::Inspect);
        }
        Ok(self.store.manifest(app, version)?)
    }

    /// Starts a switch that includes the server: checks the resulting set, records the pending group and the trial,
    /// and returns what the keeper starts. Nothing activates here; the new generation applies the group.
    pub fn begin_server_switch(
        &self,
        targets: &[(AppId, Version)],
    ) -> Result<ServerUpgrade, ReleaseError> {
        self.leading()?;
        let mut state = self.lock();
        if let Some(trial) = state
            .server
            .trial
            .clone()
            .or_else(|| state.pending_group.as_ref().map(|g| g.server.1.clone()))
            .or_else(|| state.rollout.as_ref().map(|r| r.target.clone()))
        {
            return Err(ReleaseError::UpgradeInProgress(trial));
        }
        let server_app = self.server.app.clone();
        let mut manifests = self.manifests_of(&state);
        let mut server_target = None;
        let mut apps = Vec::new();
        for (app, version) in targets {
            self.policy(app)?;
            if !state
                .apps
                .get(app)
                .is_some_and(|current| current.kept.contains(version))
            {
                return Err(ReleaseError::NotKept(app.clone(), version.clone()));
            }
            manifests.insert(app.clone(), self.manifest_for(app, version)?);
            if *app == server_app {
                server_target = Some(version.clone());
            } else {
                apps.push((
                    app.clone(),
                    state.apps.get(app).and_then(|c| c.active.clone()),
                    version.clone(),
                ));
            }
        }
        let version = server_target.ok_or(ReleaseError::ServerSwitchViaKeeper)?;
        crate::compat::check_set(&manifests, &self.php_runtimes)
            .map_err(ReleaseError::Incompatible)?;

        let binary = self
            .store
            .release_dir(&server_app, &version)
            .join(SERVER_BINARY);
        if !self.cluster.is_single() {
            let (_, live) = self
                .cluster
                .members_now()
                .ok_or(ReleaseError::MembersUnreadable)?;
            let order = crate::rollout::leader_last(&live, &self.cluster.pod);
            let previous = state
                .server
                .confirmed
                .clone()
                .unwrap_or_else(|| self.server.version.clone());
            let group = (!apps.is_empty()).then(|| PendingGroup {
                server: (previous.clone(), version.clone()),
                apps,
            });
            let mut next = state.clone();
            next.rollout = Some(speedyspec_server_domain::Rollout {
                id: state.revision + 1,
                target: version.clone(),
                previous,
                group,
                order: order.clone(),
                done: Vec::new(),
                turn: None,
                reverting: false,
                failure: None,
            });
            next.server.trial = Some(version.clone());
            self.commit_state(&mut state, next)?;
            tracing::info!(event = "rollout_requested", version = %version, order = ?order);
            return Ok(ServerUpgrade {
                version,
                binary,
                rollout: Some(order),
            });
        }
        let mut next = state.clone();
        next.pending_group = Some(PendingGroup {
            server: (self.server.version.clone(), version.clone()),
            apps,
        });
        next.server.trial = Some(version.clone());
        self.commit_state(&mut state, next)?;
        tracing::info!(event = "upgrade_requested", version = %version);
        Ok(ServerUpgrade {
            rollout: None,
            binary: self
                .store
                .release_dir(&server_app, &version)
                .join(SERVER_BINARY),
            version,
        })
    }

    /// In a new generation's start-up: when the pending group's server target is this binary, switches the group's
    /// apps to their targets before the generation reports ready.
    pub fn apply_pending_group(&self, own_version: &Version) -> Result<(), ReleaseError> {
        if !self.cluster.is_single() {
            return self.apply_rollout_group(own_version);
        }
        let mut state = self.lock();
        let Some(group) = state.pending_group.clone() else {
            return Ok(());
        };
        if group.server.1 != *own_version {
            return Ok(());
        }
        let targets: Vec<(AppId, Version)> = group
            .apps
            .iter()
            .map(|(app, _, target)| (app.clone(), target.clone()))
            .collect();
        self.switch_locked(&mut state, &targets)?;
        let mut next = state.clone();
        next.app_mut(&self.server.app).active = Some(own_version.clone());
        self.commit_state(&mut state, next)
    }

    /// After a failed trial: puts every group member's state back to its previous version, clears the trial and the
    /// group, and records the failure. Reads the state from disk first, because the failed generation changed it.
    /// Only the state changes: a resumed generation still serves the previous releases, and a fresh one activates
    /// what the state now says.
    pub fn revert_pending_group(&self, failed: &Version, reason: &str) -> Result<(), ReleaseError> {
        if !self.cluster.is_single() {
            // In a cluster the trial is this pod's own: record it in the member file; the leader reverts the rollout.
            let (failed, reason) = (failed.clone(), reason.to_string());
            let rollout = self.snapshot().rollout.map(|r| r.id).unwrap_or(0);
            self.publish(|node| {
                node.report.server.failed_in.insert(failed.clone(), rollout);
                node.report.server.failed.insert(failed, reason);
                node.report.server.trial = None;
            });
            return Ok(());
        }
        let mut state = self.lock();
        *state = self.state_store.load()?;
        self.revert_locked(&mut state, failed, reason)
    }

    /// The keeper refused a server switch this generation recorded: clears the trial and the pending group for
    /// `version` without recording a failure. Nothing was activated, so nothing else changes.
    pub fn cancel_pending_group(&self, version: &Version) -> Result<(), ReleaseError> {
        let mut state = self.lock();
        *state = self.state_store.load()?;
        let mut next = state.clone();
        if next
            .pending_group
            .as_ref()
            .is_some_and(|g| g.server.1 == *version)
        {
            next.pending_group = None;
        }
        if next.server.trial.as_ref() == Some(version) {
            next.server.trial = None;
        }
        if next == *state {
            return Ok(());
        }
        self.commit_state(&mut state, next)?;
        tracing::info!(event = "upgrade_cancelled", version = %version);
        Ok(())
    }

    /// At the end of a successful trial, in the generation that passed it: confirms its version and clears the trial
    /// and the pending group, keeping this manager's state in step with `state.json`.
    pub fn confirm_server_trial(&self, version: &Version) -> Result<(), ReleaseError> {
        if !self.cluster.is_single() {
            let version = version.clone();
            self.publish(|node| {
                node.report.server.confirmed = Some(version.clone());
                node.report.server.running = Some(version);
                node.report.server.trial = None;
            });
            return Ok(());
        }
        let mut state = self.lock();
        let mut next = state.clone();
        next.server.confirmed = Some(version.clone());
        next.server.trial = None;
        next.pending_group = None;
        next.app_mut(&self.server.app).active = Some(version.clone());
        self.commit_state(&mut state, next)?;
        tracing::info!(event = "server_confirmed", version = %version);
        Ok(())
    }

    /// The newest kept server binary older than the running one: the default target of `rollback server`.
    pub fn previous_server(&self) -> Option<Version> {
        let state = self.snapshot();
        state
            .apps
            .get(&self.server.app)?
            .kept
            .iter()
            .filter(|version| **version < self.server.version)
            .max()
            .cloned()
    }

    /// The server app's id.
    pub fn server_app(&self) -> &AppId {
        &self.server.app
    }

    fn revert_locked(
        &self,
        state: &mut ReleaseState,
        failed: &Version,
        reason: &str,
    ) -> Result<(), ReleaseError> {
        let mut next = state.clone();
        if let Some(group) = next.pending_group.take() {
            for (app, previous, _) in &group.apps {
                next.app_mut(app).active = previous.clone();
            }
            next.app_mut(&self.server.app).active = Some(group.server.0.clone());
        }
        next.server.trial = None;
        next.server
            .failed
            .insert(failed.clone(), reason.to_string());
        self.commit_state(state, next)?;
        tracing::warn!(event = "trial_reverted", version = %failed, reason = reason);
        Ok(())
    }
}

/// At the end of a successful trial (called by the keeper, which has no release manager): confirms the version,
/// makes it the server app's active release, and clears the trial and the pending group.
pub fn confirm_server(
    state_store: &dyn StateStore,
    app: &AppId,
    version: &Version,
) -> Result<(), StateError> {
    let mut state = state_store.load()?;
    state.revision += 1;
    state.server.confirmed = Some(version.clone());
    state.server.trial = None;
    state.pending_group = None;
    state.app_mut(app).active = Some(version.clone());
    state_store.save(&state)
}

/// Start-up in managed mode: keeps the running executable as a server release and confirms it on first start, or
/// when it is newer than the confirmed binary and has not failed (a package upgraded in place). Otherwise does
/// nothing. Returns the confirmed binary's path.
pub fn register_running_binary(
    store: &dyn ReleaseStore,
    state_store: &dyn StateStore,
    app: &AppId,
    version: &Version,
    exe: &std::path::Path,
) -> Result<PathBuf, ReleaseError> {
    let mut state = state_store.load()?;
    if let Some(confirmed) = &state.server.confirmed {
        let path = store.release_dir(app, confirmed).join(SERVER_BINARY);
        let adopt = version > confirmed && !state.server.failed.contains_key(version);
        if path.exists() && !adopt {
            return Ok(path);
        }
    }
    let dir = store.release_dir(app, version);
    std::fs::create_dir_all(&dir).map_err(|err| store_error(&dir, err))?;
    let path = dir.join(SERVER_BINARY);
    std::fs::copy(exe, &path).map_err(|err| store_error(&path, err))?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .map_err(|err| store_error(&path, err))?;
    }
    let current = state.app_mut(app);
    if !current.kept.contains(version) {
        current.kept.push(version.clone());
    }
    current.active = Some(version.clone());
    state.server.confirmed = Some(version.clone());
    state.revision += 1;
    state_store.save(&state)?;
    Ok(path)
}

pub(crate) fn store_error(path: &std::path::Path, err: impl ToString) -> StoreError {
    StoreError {
        path: PathBuf::from(path),
        message: err.to_string(),
    }
}

/// Milliseconds since the Unix epoch, for display fields.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Whether `report` answered (prepared or failed) the staged `revision`.
fn answered(report: &MemberReport, revision: u64) -> bool {
    report.prepared == Some(revision)
        || report
            .prepare_failed
            .as_ref()
            .is_some_and(|(r, _)| *r == revision)
}

impl ReleaseManager {
    /// Records that this pod now serves `app` at `version`.
    pub(crate) fn served(&self, app: &AppId, version: &Version) {
        self.node
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .serving
            .insert(app.clone(), version.clone());
    }

    /// Activates a prepared release and records it as served.
    fn go_live(&self, kind: AppKind, ready: Prepared) {
        self.served(&ready.app, &ready.version);
        if let Some(runtime) = self.runtime(kind) {
            runtime.activate(ready);
        }
    }

    /// Publishes this pod's member report after `change`, with a new heartbeat. Failures are logged.
    pub(crate) fn publish(&self, change: impl FnOnce(&mut Node)) {
        let mut node = self.node.lock().unwrap_or_else(|p| p.into_inner());
        change(&mut node);
        let mut holds: Vec<(AppId, Version)> = node
            .serving
            .iter()
            .map(|(app, version)| (app.clone(), version.clone()))
            .collect();
        if let Some((_, held)) = &node.held {
            holds.extend(
                held.iter()
                    .map(|(_, ready)| (ready.app.clone(), ready.version.clone())),
            );
        }
        holds.sort();
        holds.dedup();
        node.report.holds = holds;
        node.report.pod = self.cluster.pod.clone();
        node.report.heartbeat += 1;
        if let Err(err) = self.cluster.members.write(&node.report) {
            tracing::warn!(event = "member_write_failed", error = %err);
        }
    }

    /// Publishes this pod's member report unchanged, with a new heartbeat.
    pub fn heartbeat(&self) {
        self.publish(|_| {});
    }

    /// Every release a live member serves or holds prepared.
    fn held_by_live_members(&self) -> Option<std::collections::BTreeSet<(AppId, Version)>> {
        let (reports, live) = self.cluster.members_now()?;
        let mut held: std::collections::BTreeSet<(AppId, Version)> = reports
            .iter()
            .filter(|report| live.contains(&report.pod))
            .flat_map(|report| report.holds.iter().cloned())
            .collect();
        let node = self.node.lock().unwrap_or_else(|p| p.into_inner());
        held.extend(
            node.serving
                .iter()
                .map(|(app, version)| (app.clone(), version.clone())),
        );
        Some(held)
    }

    /// Prepares every target through its runtime, without activating any.
    fn prepare_targets(
        &self,
        targets: &[(AppId, Version)],
    ) -> (Vec<(AppKind, Prepared)>, Vec<(AppId, Version, String)>) {
        let mut prepared = Vec::new();
        let mut failures = Vec::new();
        for (app, version) in targets {
            let result = self
                .store
                .manifest(app, version)
                .map_err(|err| err.to_string())
                .and_then(|manifest| match self.runtime(manifest.kind) {
                    Some(runtime) => runtime
                        .prepare(app, &manifest, &self.store.release_dir(app, version))
                        .map(|ready| (manifest.kind, ready)),
                    None => Err(format!("no runtime for kind {}", manifest.kind.as_str())),
                });
            match result {
                Ok(ready) => prepared.push(ready),
                Err(reason) => failures.push((app.clone(), version.clone(), reason)),
            }
        }
        (prepared, failures)
    }

    /// The leader's two-step switch: stage the set, prepare it here, wait for every live pod to report on it (or
    /// `prepare_timeout`), then make it active if any pod prepared it, or abandon it and mark it failed.
    fn switch_staged(
        &self,
        state: &mut ReleaseState,
        targets: &[(AppId, Version)],
    ) -> Result<(), ReleaseError> {
        for (app, version) in targets {
            if self.policy(app)?.kind == AppKind::Server {
                return Err(ReleaseError::ServerSwitchViaKeeper);
            }
            if !state
                .apps
                .get(app)
                .is_some_and(|current| current.kept.contains(version))
            {
                return Err(ReleaseError::NotKept(app.clone(), version.clone()));
            }
        }
        let targets: Vec<(AppId, Version)> = targets
            .iter()
            .filter(|(app, version)| {
                state.apps.get(app).and_then(|c| c.active.as_ref()) != Some(version)
            })
            .cloned()
            .collect();
        if targets.is_empty() {
            return Ok(());
        }
        let mut manifests = self.manifests_of(state);
        for (app, version) in &targets {
            manifests.insert(app.clone(), self.store.manifest(app, version)?);
        }
        check_set(&manifests, &self.php_runtimes).map_err(ReleaseError::Incompatible)?;

        let revision = state.revision + 1;
        let mut next = state.clone();
        next.staged = Some(StagedSet {
            revision,
            targets: targets.clone(),
            since_ms: now_ms(),
        });
        self.commit_state(state, next)?;
        tracing::info!(event = "staged", revision, targets = ?targets.iter().map(|(a, v)| format!("{a}@{v}")).collect::<Vec<_>>());

        let (prepared, failures) = self.prepare_targets(&targets);
        let prepared_ok = failures.is_empty();
        self.publish(|node| {
            if prepared_ok {
                node.held = Some((revision, prepared));
                node.report.prepared = Some(revision);
            } else {
                let (app, version, reason) = &failures[0];
                node.report.prepare_failed = Some((revision, format!("{app} {version}: {reason}")));
            }
        });
        tracing::info!(event = "prepared", pod = %self.cluster.pod, revision, ok = prepared_ok);

        let failure = failures.first().map(|(_, _, reason)| reason.clone());
        self.finish_staged(state, revision, &targets, failure)
    }

    /// Waits for every live pod to report on the staged `revision` (or `prepare_timeout`), then makes the set active
    /// if any pod prepared it, or abandons it and marks it failed. `my_failure` is this pod's own prepare failure.
    fn finish_staged(
        &self,
        state: &mut ReleaseState,
        revision: u64,
        targets: &[(AppId, Version)],
        my_failure: Option<String>,
    ) -> Result<(), ReleaseError> {
        let mine_ok = my_failure.is_none();
        let deadline = self.cluster.now() + self.cluster.prepare_timeout;
        let (reports, live) = loop {
            self.leading()?;
            let Some((reports, live)) = self.cluster.members_now() else {
                // Who has prepared is unknown: keep waiting, up to the timeout.
                if self.cluster.now() >= deadline {
                    break (Vec::new(), vec![self.cluster.pod.clone()]);
                }
                std::thread::sleep(self.cluster.poll_interval);
                continue;
            };
            let waiting: Vec<&String> = live
                .iter()
                .filter(|pod| **pod != self.cluster.pod)
                .filter(|pod| {
                    !reports
                        .iter()
                        .any(|r| r.pod == **pod && answered(r, revision))
                })
                .collect();
            if waiting.is_empty() || self.cluster.now() >= deadline {
                break (reports, live);
            }
            std::thread::sleep(self.cluster.poll_interval);
        };
        let mut ready_pods = Vec::new();
        let mut failed_pods = Vec::new();
        if mine_ok {
            ready_pods.push(self.cluster.pod.clone());
        } else {
            failed_pods.push((
                self.cluster.pod.clone(),
                my_failure.clone().unwrap_or_default(),
            ));
        }
        for report in reports
            .iter()
            .filter(|r| r.pod != self.cluster.pod && live.contains(&r.pod))
        {
            if report.prepared == Some(revision) {
                ready_pods.push(report.pod.clone());
            } else if let Some((r, reason)) = &report.prepare_failed
                && *r == revision
            {
                failed_pods.push((report.pod.clone(), reason.clone()));
            }
        }

        // A release that migrates does so once, here on the leader, after every pod has prepared and before the set
        // is made active: no pod can switch to it before the migration is done.
        let mut scopes = Vec::new();
        if !ready_pods.is_empty() {
            let held = self
                .node
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .held
                .take();
            let result = match &held {
                Some((r, prepared)) if *r == revision => self.migrate_all(state, prepared, true),
                _ if self.migrates(targets) => Err(ReleaseError::Prepare {
                    app: targets[0].0.clone(),
                    version: targets[0].1.clone(),
                    reason: "the leader did not prepare the set, so it cannot run its migration"
                        .to_string(),
                }),
                _ => Ok(Vec::new()),
            };
            self.node.lock().unwrap_or_else(|p| p.into_inner()).held = held;
            match result {
                Ok(done) => scopes = done,
                Err(err) => {
                    let mut next = state.clone();
                    next.staged = None;
                    self.commit_state(state, next)?;
                    self.publish(|node| node.held = None);
                    tracing::error!(event = "switch_abandoned", revision, reason = %err);
                    return Err(err);
                }
            }
        }
        let mut next = state.clone();
        next.staged = None;
        if ready_pods.is_empty() {
            let reason = failed_pods
                .iter()
                .map(|(pod, why)| format!("{pod}: {why}"))
                .collect::<Vec<_>>()
                .join("; ");
            let reason = if reason.is_empty() {
                "no pod prepared it in time".to_string()
            } else {
                reason
            };
            for (app, version) in targets {
                next.app_mut(app)
                    .failed
                    .insert(version.clone(), reason.clone());
            }
            self.commit_state(state, next)?;
            self.publish(|node| node.held = None);
            tracing::error!(event = "switch_abandoned", revision, reason = %reason);
            let (app, version) = targets[0].clone();
            return Err(ReleaseError::Prepare {
                app,
                version,
                reason,
            });
        }
        let old: Vec<(AppId, Option<Version>)> = targets
            .iter()
            .map(|(app, _)| {
                (
                    app.clone(),
                    state.apps.get(app).and_then(|c| c.active.clone()),
                )
            })
            .collect();
        for (app, version) in targets {
            let current = next.app_mut(app);
            current.previous = current.active.replace(version.clone());
            current.failed.remove(version);
        }
        self.commit_state(state, next)?;
        let held = self
            .node
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .held
            .take();
        if let Some((_, held)) = held.filter(|(r, _)| *r == revision) {
            for (kind, ready) in held {
                self.go_live(kind, ready);
            }
            for (app, previous) in &old {
                if let (Some(previous), Ok(policy)) = (previous, self.policy(app))
                    && let Some(runtime) = self.runtime(policy.kind)
                {
                    runtime.retire(app, previous);
                }
            }
        }
        let serving = state.revision;
        self.publish(|node| {
            node.report.serving = serving;
            node.report.ready = mine_ok;
        });
        tracing::info!(event = "activated", revision = serving, prepared = ?ready_pods, failed = ?failed_pods);
        self.finish_migrations(state, &scopes)?;
        let apps: Vec<AppId> = targets.iter().map(|(app, _)| app.clone()).collect();
        self.prune(state, &apps)?;
        Ok(())
    }

    /// One follower step: picks up `state.json`, prepares a staged set it has not answered, and serves the active set
    /// (keeping the old one, and reporting not ready, when the active set does not prepare).
    pub fn follow(&self) -> Result<FollowOutcome, ReleaseError> {
        *self.took_over.lock().unwrap_or_else(|p| p.into_inner()) = None;
        let changed = self.refresh()?;
        let state = self.snapshot();
        let serving = self
            .node
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .serving
            .clone();
        let differs = |targets: &[(AppId, Version)]| -> Vec<(AppId, Version)> {
            targets
                .iter()
                .filter(|(app, version)| serving.get(app) != Some(version))
                .cloned()
                .collect()
        };

        if let Some(staged) = &state.staged {
            let done = answered(
                &self.node.lock().unwrap_or_else(|p| p.into_inner()).report,
                staged.revision,
            );
            if !done {
                let (prepared, failures) = self.prepare_targets(&differs(&staged.targets));
                let revision = staged.revision;
                self.publish(|node| match failures.first() {
                    None => {
                        node.held = Some((revision, prepared));
                        node.report.prepared = Some(revision);
                        node.report.prepare_failed = None;
                    }
                    Some((app, version, reason)) => {
                        node.held = None;
                        node.report.prepare_failed =
                            Some((revision, format!("{app} {version}: {reason}")));
                    }
                });
                tracing::info!(event = "prepared", pod = %self.cluster.pod, revision, ok = failures.is_empty());
            }
        }

        let wanted: Vec<(AppId, Version)> = state
            .apps
            .iter()
            .filter(|(app, _)| self.policy(app).is_ok_and(|p| p.kind != AppKind::Server))
            .filter_map(|(app, current)| {
                current.active.clone().map(|version| (app.clone(), version))
            })
            .collect();
        // A pod already running the rollout's target serves the group's apps with it, until the rollout ends.
        let wanted: Vec<(AppId, Version)> = match state.rollout.as_ref().and_then(|r| {
            (r.target == self.server.version)
                .then_some(r.group.as_ref())
                .flatten()
        }) {
            Some(group) => wanted
                .into_iter()
                .map(|(app, version)| {
                    let target = group
                        .apps
                        .iter()
                        .find(|(a, _, _)| *a == app)
                        .map(|(_, _, t)| t.clone());
                    (app, target.unwrap_or(version))
                })
                .collect(),
            None => wanted,
        };
        let missing = differs(&wanted);
        let mut ready = true;
        if !missing.is_empty() {
            let mut held = self
                .node
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .held
                .take()
                .map(|(_, h)| h)
                .unwrap_or_default();
            let mut go = Vec::new();
            let mut unheld = Vec::new();
            for (app, version) in &missing {
                match held
                    .iter()
                    .position(|(_, ready)| ready.app == *app && ready.version == *version)
                {
                    Some(index) => go.push(held.swap_remove(index)),
                    None => unheld.push((app.clone(), version.clone())),
                }
            }
            let (prepared, failures) = self.prepare_targets(&unheld);
            if let Some((app, version, reason)) = failures.first() {
                ready = false;
                tracing::error!(event = "active_set_not_prepared", pod = %self.cluster.pod, app = %app, version = %version, reason = %reason);
            } else {
                go.extend(prepared);
                for (kind, ready) in go {
                    let previous = serving.get(&ready.app).cloned();
                    let app = ready.app.clone();
                    self.go_live(kind, ready);
                    if let (Some(previous), Some(runtime)) = (previous, self.runtime(kind)) {
                        runtime.retire(&app, &previous);
                    }
                }
                tracing::info!(event = "activated", pod = %self.cluster.pod, revision = state.revision);
            }
        }
        let revision = state.revision;
        self.publish(|node| {
            if ready {
                node.report.serving = revision;
            }
            node.report.ready = ready;
        });
        Ok(FollowOutcome { changed, ready })
    }
}

/// What a new leader did with the state it inherited.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TakeOver {
    /// The staged revision it carried on and made active.
    pub finished: Option<u64>,
    /// The staged revision it abandoned, and why.
    pub abandoned: Option<(u64, String)>,
}

impl ReleaseManager {
    /// Called once when this pod becomes the leader: deletes stale partial downloads, picks up `state.json` and the
    /// member files, and finishes a staged switch from where it stands, or abandons it when a target can no longer be
    /// verified in the store.
    pub fn take_over(&self) -> Result<TakeOver, ReleaseError> {
        self.leading()?;
        // Partials are a previous leader's leftovers, unless this pod is downloading right now (it led before and
        // leads again): then they are its own and stay.
        let downloading = self
            .downloads
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .any(|guard| guard.try_lock().is_err());
        let cleaned = if downloading {
            Ok(Vec::new())
        } else {
            self.store.clean_partials()
        };
        match cleaned {
            Ok(cleaned) if !cleaned.is_empty() => {
                tracing::info!(event = "partials_cleaned", count = cleaned.len())
            }
            Ok(_) => {}
            Err(err) => tracing::warn!(event = "clean_partials_failed", error = %err),
        }
        // The state the previous leader left carries its older epoch: inherit it, then re-stamp it with this
        // leader's epoch, so followers (which ignore older epochs) take it up again.
        self.reload()?;
        let _ = self.cluster.live_members();
        let mut state = self.lock();
        // Stamp above an inherited epoch higher than the Lease's (a recreated Lease starts over at 0): followers ignore
        // anything older than what they have seen. An equal epoch is this leader's own write.
        let lease_epoch = self.cluster.view().epoch;
        if state.epoch > lease_epoch {
            self.cluster
                .epoch_floor
                .fetch_max(state.epoch + 1, std::sync::atomic::Ordering::SeqCst);
        }
        let restamped = state.clone();
        self.commit_state(&mut state, restamped)?;
        let mut outcome = TakeOver::default();
        let Some(staged) = state.staged.clone() else {
            return Ok(outcome);
        };
        tracing::info!(event = "take_over_staged", revision = staged.revision);
        let unverifiable = staged
            .targets
            .iter()
            .find(|(app, version)| self.store.manifest(app, version).is_err());
        if let Some((app, version)) = unverifiable {
            let reason = format!("the new leader could not verify {app} {version}");
            let mut next = state.clone();
            next.staged = None;
            self.commit_state(&mut state, next)?;
            tracing::error!(event = "switch_abandoned", revision = staged.revision, reason = %reason);
            outcome.abandoned = Some((staged.revision, reason));
            return Ok(outcome);
        }
        let serving = self
            .node
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .serving
            .clone();
        let unserved: Vec<(AppId, Version)> = staged
            .targets
            .iter()
            .filter(|(app, version)| serving.get(app) != Some(version))
            .cloned()
            .collect();
        let (prepared, failures) = self.prepare_targets(&unserved);
        let failure = failures
            .first()
            .map(|(app, version, reason)| format!("{app} {version}: {reason}"));
        let revision = staged.revision;
        self.publish(|node| match &failure {
            None => {
                node.held = Some((revision, prepared));
                node.report.prepared = Some(revision);
            }
            Some(reason) => node.report.prepare_failed = Some((revision, reason.clone())),
        });
        self.finish_staged(&mut state, revision, &staged.targets, failure)?;
        outcome.finished = Some(revision);
        Ok(outcome)
    }
}

impl ReleaseManager {
    /// On the leader: rewrites `state.json` from memory when a write stamped with an older epoch (a deposed leader's,
    /// landing late) has replaced it. Returns whether it rewrote.
    pub fn repair(&self) -> Result<bool, ReleaseError> {
        self.leading()?;
        let on_disk = self.state_store.load()?;
        let view = self.cluster.view();
        if on_disk.epoch >= self.cluster.stamp(view.epoch) {
            return Ok(false);
        }
        let mut state = self.lock();
        tracing::warn!(
            event = "stale_epoch_repaired",
            found = on_disk.epoch,
            epoch = view.epoch
        );
        let current = state.clone();
        self.commit_state(&mut state, current)?;
        Ok(true)
    }
}

impl ReleaseManager {
    /// Publishes this pod's own server binary state in its member report (a cluster's per-pod trial bookkeeping).
    pub fn report_server(&self, server: speedyspec_server_domain::PodServer) {
        self.publish(|node| node.report.server = server);
    }

    /// The leader's rollout step: decides the next step from the member reports and records it. `None` when no
    /// rollout is running.
    pub fn advance_rollout(&self) -> Result<Option<crate::RolloutStep>, ReleaseError> {
        use crate::RolloutStep;

        self.leading()?;
        let mut state = self.lock();
        let Some(rollout) = state.rollout.clone() else {
            return Ok(None);
        };
        let Some((reports, live)) = self.cluster.members_now() else {
            return Ok(Some(crate::RolloutStep::Wait));
        };
        let mut step = crate::rollout::next_step(&rollout, &live, &reports);
        if let (crate::RolloutStep::Wait, Some(turn)) = (&step, rollout.turn.as_ref()) {
            let now = self.cluster.now();
            let mut clock = self
                .cluster
                .turn_clock
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            match clock.as_ref() {
                Some((pod, since)) if pod == turn => {
                    if !rollout.reverting
                        && now.saturating_duration_since(*since) >= self.cluster.turn_timeout
                    {
                        step = crate::RolloutStep::Failed {
                            pod: turn.clone(),
                            reason: format!(
                                "did not finish its turn within {} s",
                                self.cluster.turn_timeout.as_secs()
                            ),
                        };
                    }
                }
                _ => *clock = Some((turn.clone(), now)),
            }
        }
        let mut next = state.clone();
        let finished_turn = rollout.turn.clone().filter(|turn| {
            !rollout.reverting && crate::rollout::finished(&reports, turn, &rollout.target)
        });
        match &step {
            RolloutStep::Wait => return Ok(Some(step)),
            RolloutStep::Turn(pod) => {
                let plan = next.rollout.as_mut().expect("checked above");
                if let Some(turn) = finished_turn
                    && !plan.done.contains(&turn)
                {
                    plan.done.push(turn);
                }
                if !plan.order.contains(pod) {
                    plan.order.push(pod.clone());
                }
                plan.turn = Some(pod.clone());
                let reverting = plan.reverting;
                if next == *state {
                    return Ok(Some(step));
                }
                let plan = next.rollout.as_ref().expect("checked above");
                tracing::info!(event = "rollout_step", pod = %pod, reverting, version = %if reverting { &plan.previous } else { &plan.target });
            }
            RolloutStep::Done => {
                next.rollout = None;
                next.server.confirmed = Some(rollout.target.clone());
                next.server.trial = None;
                next.app_mut(&self.server.app).active = Some(rollout.target.clone());
                if let Some(group) = &rollout.group {
                    for (app, _, target) in &group.apps {
                        let current = next.app_mut(app);
                        current.previous = current.active.replace(target.clone());
                        current.failed.remove(target);
                    }
                }
                tracing::info!(event = "rollout_step", done = true, version = %rollout.target);
            }
            RolloutStep::Failed { pod, reason } => {
                let plan = next.rollout.as_mut().expect("checked above");
                plan.reverting = true;
                plan.turn = None;
                plan.failure = Some((pod.clone(), reason.clone()));
                tracing::error!(event = "rollout_failed", pod = %pod, version = %rollout.target, reason = %reason);
            }
            RolloutStep::Reverted => {
                let (pod, reason) = rollout.failure.clone().unwrap_or_default();
                next.rollout = None;
                next.server.trial = None;
                next.server
                    .failed
                    .insert(rollout.target.clone(), format!("{pod}: {reason}"));
                tracing::error!(event = "rollout_reverted", version = %rollout.target, pod = %pod);
            }
        }
        self.commit_state(&mut state, next)?;
        Ok(Some(step))
    }
}

/// One member as `status` shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberStatus {
    pub pod: String,
    pub live: bool,
    pub serving: u64,
    pub ready: bool,
    pub server: Option<Version>,
}

/// This pod's place in the cluster, as `status` shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClusterStatus {
    pub pod: String,
    pub leader: bool,
    pub leader_name: Option<String>,
    pub members: Vec<MemberStatus>,
}

impl ReleaseManager {
    /// After `boot`: this pod serves the active set it booted into, and reports itself ready.
    pub fn booted(&self, running: &Version) {
        let revision = self.snapshot().revision;
        let running = running.clone();
        let state = self.snapshot();
        let catching_up =
            state.rollout.is_none() && state.server.confirmed.as_ref() == Some(&running);
        // A pod's server state outlives its generations: start from this pod's last report.
        let previous = (!self.cluster.is_single())
            .then(|| self.cluster.members.read_all().ok())
            .flatten()
            .and_then(|reports| reports.into_iter().find(|r| r.pod == self.cluster.pod));
        self.publish(|node| {
            if let Some(previous) = previous {
                node.report.heartbeat = node.report.heartbeat.max(previous.heartbeat);
                node.report.server = previous.server;
            }
            node.report.serving = revision;
            node.report.ready = true;
            let server = &mut node.report.server;
            // A pod that starts on the cluster's confirmed version, with no rollout running, missed a rollout and
            // has now caught up: that is its confirmed version.
            if catching_up {
                server.confirmed = Some(running.clone());
            }
            match &server.confirmed {
                None => server.confirmed = Some(running.clone()),
                Some(confirmed) if *confirmed != running => server.trial = Some(running.clone()),
                Some(_) => server.trial = None,
            }
            server.running = Some(running);
        });
    }

    /// Whether this pod serves the active set (the readiness probe's answer).
    pub fn is_ready(&self) -> bool {
        self.node
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .report
            .ready
    }

    /// `None` on a single host.
    pub fn cluster_status(&self) -> Option<ClusterStatus> {
        if self.cluster.is_single() {
            return None;
        }
        let view = self.cluster.view();
        let (reports, live) = self.cluster.live_members();
        Some(ClusterStatus {
            pod: self.cluster.pod.clone(),
            leader: view.is_leader,
            leader_name: view.leader,
            members: reports
                .iter()
                .map(|report| MemberStatus {
                    pod: report.pod.clone(),
                    live: live.contains(&report.pod),
                    serving: report.serving,
                    ready: report.ready,
                    server: report.server.running.clone(),
                })
                .collect(),
        })
    }
}

impl ReleaseManager {
    /// In a cluster, a new generation on the rollout's target serves the group's apps in memory; `state.json` makes
    /// them active only once the whole rollout is confirmed.
    fn apply_rollout_group(&self, own_version: &Version) -> Result<(), ReleaseError> {
        let state = self.snapshot();
        let Some(group) = state
            .rollout
            .as_ref()
            .filter(|r| r.target == *own_version)
            .and_then(|r| r.group.clone())
        else {
            return Ok(());
        };
        let targets: Vec<(AppId, Version)> = group
            .apps
            .iter()
            .map(|(app, _, t)| (app.clone(), t.clone()))
            .collect();
        let (prepared, failures) = self.prepare_targets(&targets);
        if let Some((app, version, reason)) = failures.into_iter().next() {
            return Err(ReleaseError::Prepare {
                app,
                version,
                reason,
            });
        }
        for (kind, ready) in prepared {
            self.go_live(kind, ready);
        }
        Ok(())
    }

    /// In a cluster: when the rollout's turn is this pod and it does not run the turn's version yet, that version and
    /// its kept binary.
    pub fn my_turn(&self) -> Option<(Version, PathBuf)> {
        let state = self.snapshot();
        let rollout = state.rollout.as_ref()?;
        if rollout.turn.as_deref() != Some(self.cluster.pod.as_str()) {
            return None;
        }
        let want = if rollout.reverting {
            &rollout.previous
        } else {
            &rollout.target
        };
        if *want == self.server.version {
            return None;
        }
        let binary = self
            .store
            .release_dir(&self.server.app, want)
            .join(SERVER_BINARY);
        if !binary.exists() {
            tracing::error!(event = "rollout_binary_missing", version = %want, path = %binary.display());
            return None;
        }
        Some((want.clone(), binary))
    }

    /// On the leader in a cluster: keeps the running executable as this version's server release (if it is not kept
    /// yet) and makes it the confirmed one when none is, so every pod has a binary to fall back or revert to.
    pub fn keep_running_binary(&self, exe: &std::path::Path) -> Result<(), ReleaseError> {
        self.leading()?;
        // The previous leader may have written since this pod last read the state.
        self.reload()?;
        let mut state = self.lock();
        let version = self.server.version.clone();
        let app = self.server.app.clone();
        let path = self.store.release_dir(&app, &version).join(SERVER_BINARY);
        let kept = state
            .apps
            .get(&app)
            .is_some_and(|a| a.kept.contains(&version));
        if kept && path.exists() && state.server.confirmed.is_some() {
            return Ok(());
        }
        if !path.exists() {
            let dir = self.store.release_dir(&app, &version);
            std::fs::create_dir_all(&dir).map_err(|err| store_error(&dir, err))?;
            std::fs::copy(exe, &path).map_err(|err| store_error(&path, err))?;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .map_err(|err| store_error(&path, err))?;
        }
        let mut next = state.clone();
        let current = next.app_mut(&app);
        if !current.kept.contains(&version) {
            current.kept.push(version.clone());
        }
        current.active.get_or_insert(version.clone());
        next.server.confirmed.get_or_insert(version.clone());
        self.commit_state(&mut state, next)?;
        tracing::info!(event = "server_binary_kept", version = %version);
        Ok(())
    }
}

impl ReleaseManager {
    /// One leader step. On becoming leader (and again after any failure, until it succeeds): take over the
    /// inherited state, then keep the running binary as a release. Only after that: repair a late-written state, and
    /// advance the rollout.
    pub fn lead(&self, exe: Option<&std::path::Path>) -> Result<(), ReleaseError> {
        self.leading()?;
        let epoch = self.cluster.view().epoch;
        let current = *self.took_over.lock().unwrap_or_else(|p| p.into_inner());
        if current != Some(epoch) {
            let outcome = self.take_over()?;
            tracing::info!(event = "took_over", finished = ?outcome.finished, abandoned = ?outcome.abandoned);
            if let Some(exe) = exe {
                self.keep_running_binary(exe)?;
            }
            *self.took_over.lock().unwrap_or_else(|p| p.into_inner()) = Some(epoch);
        }
        self.repair()?;
        self.advance_rollout()?;
        Ok(())
    }
}
