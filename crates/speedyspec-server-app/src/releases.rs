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

use crate::compat::Incompatibility;
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

        let current = state.app_mut(app);
        current.kept.retain(|kept| *kept != version);
        current.kept.push(version.clone());
        current.failed.remove(&version);
        state.revision += 1;
        self.state_store.save(&state)?;
        tracing::info!(event = "installed", app = %app, version = %version);

        Ok(version)
    }
}

pub(crate) fn store_error(path: &std::path::Path, err: impl ToString) -> StoreError {
    StoreError {
        path: PathBuf::from(path),
        message: err.to_string(),
    }
}
