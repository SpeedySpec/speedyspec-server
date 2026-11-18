//! The release lifecycle: check, install, switch, roll back, pin, prune and recover at start-up. Every operation runs
//! one at a time, under one lock that also guards the release state.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use semver::Version;
use speedyspec_server_domain::{
    AppId, AppKind, AppRuntime, IndexEntry, ReleaseManifest, ReleaseSource, ReleaseState,
    ReleaseStore, SignatureVerifier, SourceError, StateError, StateStore, StoreError,
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
    #[error("server releases need the keeper, which is not in this build yet")]
    ServerNeedsKeeper,
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
    pub(crate) state: Mutex<ReleaseState>,
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
            state: Mutex::new(state),
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
        let state = self.lock();
        let candidate = self.candidate(&state, app)?;
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
    /// candidate. A version already kept is not downloaded again.
    pub fn install(&self, app: &AppId, version: Option<Version>) -> Result<Version, ReleaseError> {
        let mut state = self.lock();
        let policy = self.policy(app)?.clone();
        if policy.kind == AppKind::Server {
            return Err(ReleaseError::ServerNeedsKeeper);
        }
        if policy.signing == Signing::NoKeys {
            return Err(ReleaseError::Unsigned(app.clone()));
        }
        if let Some(version) = &version
            && state
                .apps
                .get(app)
                .is_some_and(|current| current.kept.contains(version))
        {
            return Ok(version.clone());
        }
        let url = self.index_url(app)?.to_string();
        let entry = match version {
            Some(version) => {
                let index = self.source.index(&url)?;
                if index.app != *app {
                    return Err(SourceError {
                        url,
                        message: format!("the index is for {}, not {app}", index.app),
                    }
                    .into());
                }
                index
                    .entry(&version)
                    .cloned()
                    .ok_or_else(|| ReleaseError::NotInIndex(app.clone(), version))?
            }
            None => self
                .candidate(&state, app)?
                .ok_or_else(|| ReleaseError::NothingNewer(app.clone()))?,
        };
        let version = entry.version.clone();
        let artifact = entry
            .artifact_for("any")
            .ok_or_else(|| ReleaseError::NoArtifact(app.clone(), version.clone()))?;

        let partial = self.store.partial_dir(app, &version);
        std::fs::create_dir_all(&partial).map_err(|err| store_error(&partial, err))?;
        let archive = partial.join("artifact");
        self.source.download(&artifact.url, &archive)?;
        let actual = sha256_file(&archive).map_err(|err| store_error(&archive, err))?;
        if actual != artifact.sha256 {
            self.store.discard_partial(app, &version);
            return Err(ReleaseError::Checksum {
                app: app.clone(),
                version,
                expected: artifact.sha256.clone(),
                actual,
            });
        }
        match policy.signing {
            Signing::Verify => {
                let Some(signature_url) = &artifact.minisig else {
                    self.store.discard_partial(app, &version);
                    return Err(ReleaseError::Signature(format!(
                        "{app} {version}: the index lists no signature"
                    )));
                };
                let signature = partial.join("artifact.minisig");
                let _ = std::fs::remove_file(&signature);
                let verified = self
                    .source
                    .download(signature_url, &signature)
                    .map_err(ReleaseError::from)
                    .and_then(|()| {
                        self.verifier
                            .verify(app, &archive, &signature)
                            .map_err(|err| ReleaseError::Signature(err.0))
                    });
                if let Err(err) = verified {
                    self.store.discard_partial(app, &version);
                    return Err(err);
                }
            }
            Signing::AllowUnsigned => {
                tracing::warn!(event = "unsigned_install", app = %app, version = %version);
            }
            Signing::NoKeys => unreachable!("refused above"),
        }

        let manifest = self.store.unpack(app, &version, &archive)?;
        if manifest.kind != policy.kind {
            self.store.discard_partial(app, &version);
            return Err(store_error(
                &partial,
                format!(
                    "release kind is {}, but app.{app}.kind is {}",
                    manifest.kind.as_str(),
                    policy.kind.as_str()
                ),
            )
            .into());
        }
        self.store.commit(app, &version)?;

        let mut next = state.clone();
        let current = next.app_mut(app);
        current.kept.retain(|kept| *kept != version);
        current.kept.push(version.clone());
        current.failed.remove(&version);
        self.commit_state(&mut state, next)?;
        tracing::info!(event = "installed", app = %app, version = %version);

        Ok(version)
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
        Ok(())
    }

    fn runtime(&self, kind: AppKind) -> Option<&Arc<dyn AppRuntime>> {
        self.runtimes.get(&kind)
    }

    /// The manifest of every active release, the server's included.
    pub fn active_manifests(&self) -> BTreeMap<AppId, ReleaseManifest> {
        let state = self.lock();
        self.manifests_of(&state)
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
        self.lock().clone()
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
                return Err(ReleaseError::ServerNeedsKeeper);
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

    /// Switches back to `version`, or to the previous release. The version rolled back from is marked failed
    /// ("rolled back"), so auto-update does not offer it again; an explicit switch clears that.
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
        if let Some(from) = current.active.filter(|from| *from != target) {
            let mut next = state.clone();
            next.app_mut(app)
                .failed
                .insert(from, "rolled back".to_string());
            self.commit_state(&mut state, next)?;
        }
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

pub(crate) fn store_error(path: &std::path::Path, err: impl ToString) -> StoreError {
    StoreError {
        path: PathBuf::from(path),
        message: err.to_string(),
    }
}
