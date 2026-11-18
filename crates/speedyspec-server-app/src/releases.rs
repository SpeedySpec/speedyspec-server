//! The release lifecycle: check, install, switch, roll back, pin, prune and recover at start-up. Every operation runs
//! one at a time, under one lock that also guards the release state.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use semver::Version;
use speedyspec_server_domain::{
    AppId, AppKind, AppRuntime, BinaryInspector, IndexEntry, PendingGroup, ReleaseManifest,
    ReleaseSource, ReleaseState, ReleaseStore, SignatureVerifier, SourceError, StateError,
    StateStore, StoreError,
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
    /// The Rust target triple whose server artifacts this build installs.
    pub(crate) target: String,
    pub(crate) state: Mutex<ReleaseState>,
    /// A copy of the last saved state, read by `status` and `check` without waiting for operations.
    pub(crate) published: Mutex<ReleaseState>,
    pub(crate) downloads: Mutex<HashMap<AppId, Arc<Mutex<()>>>>,
    pub(crate) last_boot: Mutex<Option<BootReport>>,
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
    ) -> Result<ReleaseManager, ReleaseError> {
        if let Some((app, _)) = apps.iter().find(|(_, policy)| policy.keep == 0) {
            return Err(ReleaseError::KeepTooSmall(app.clone()));
        }
        let state = state_store.load()?;

        Ok(ReleaseManager {
            apps,
            server,
            source,
            verifier,
            store,
            state_store,
            runtimes,
            inspector,
            target,
            published: Mutex::new(state.clone()),
            downloads: Mutex::new(HashMap::new()),
            state: Mutex::new(state),
            last_boot: Mutex::new(None),
        })
    }

    /// Takes the operation lock. A panic in an earlier operation does not poison it for later ones.
    pub(crate) fn lock(&self) -> MutexGuard<'_, ReleaseState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
        let target = if policy.kind == AppKind::Server {
            self.target.as_str()
        } else {
            "any"
        };
        let Some(artifact) = entry.artifact_for(target) else {
            let err = if policy.kind == AppKind::Server {
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
        if self.policy(app)?.kind == AppKind::Server {
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

    /// Keeps a verified server binary: moves it into place, then reads its manifest by running it.
    fn keep_binary(
        &self,
        state: &mut ReleaseState,
        app: &AppId,
        version: &Version,
    ) -> Result<(), ReleaseError> {
        let archive = self.store.partial_dir(app, version).join("artifact");
        let path = self.store.commit_binary(app, version, &archive)?;
        let checked = self.inspector.manifest(&path).and_then(|manifest| {
            if manifest.kind != AppKind::Server || manifest.version != *version {
                Err(format!(
                    "{} reports kind {} version {}, expected a server binary of version {version}",
                    path.display(),
                    manifest.kind.as_str(),
                    manifest.version
                ))
            } else {
                Ok(())
            }
        });
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
        let mut state = self.lock();
        self.switch_locked(&mut state, targets)
    }

    pub(crate) fn switch_locked(
        &self,
        state: &mut ReleaseState,
        targets: &[(AppId, Version)],
    ) -> Result<(), ReleaseError> {
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
        check_set(&manifests).map_err(ReleaseError::Incompatible)?;

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
        for (kind, ready) in prepared {
            self.runtime(kind).expect("prepared above").activate(ready);
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
                    Some(version) != active.as_ref() && Some(version) != pinned.as_ref()
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

    pub fn unpin(&self, app: &AppId) -> Result<(), ReleaseError> {
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
        for (app, policy) in &self.apps {
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

        if let Err(incompatible) = check_set(&self.manifests_of(&state)) {
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
        let mut state = self.lock();
        if let Some(trial) = state
            .server
            .trial
            .clone()
            .or_else(|| state.pending_group.as_ref().map(|g| g.server.1.clone()))
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
        crate::compat::check_set(&manifests).map_err(ReleaseError::Incompatible)?;

        let mut next = state.clone();
        next.pending_group = Some(PendingGroup {
            server: (self.server.version.clone(), version.clone()),
            apps,
        });
        next.server.trial = Some(version.clone());
        self.commit_state(&mut state, next)?;
        tracing::info!(event = "upgrade_requested", version = %version);
        Ok(ServerUpgrade {
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
        let mut state = self.lock();
        *state = self.state_store.load()?;
        self.revert_locked(&mut state, failed, reason)
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

/// First start in managed mode: keeps the running executable as the first server release and confirms it. Does
/// nothing when a confirmed binary exists. Returns the confirmed binary's path.
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
        if path.exists() {
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
