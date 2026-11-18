//! The contracts of the release lifecycle. Implementations live in the infra crates.
//!
//! Every method is synchronous: lifecycle work runs off the async runtime, one operation at a time.

use std::any::Any;
use std::path::{Path, PathBuf};

use semver::Version;

use crate::{AppId, ReleaseIndex, ReleaseManifest, ReleaseState};

/// A source download or index fetch that failed.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("{url}: {message}")]
pub struct SourceError {
    pub url: String,
    pub message: String,
}

/// A signature that could not be checked or did not match.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("{0}")]
pub struct VerifyError(pub String);

/// A release store operation that failed.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("{}: {message}", .path.display())]
pub struct StoreError {
    pub path: PathBuf,
    pub message: String,
}

/// The state file could not be read or written.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("{}: {message}", .path.display())]
pub struct StateError {
    pub path: PathBuf,
    pub message: String,
}

/// Where releases come from.
pub trait ReleaseSource: Send + Sync {
    /// Fetches and parses the release index at `url`.
    fn index(&self, url: &str) -> Result<ReleaseIndex, SourceError>;

    /// Downloads `url` into `dest`, resuming from `dest`'s current length, and starting again from zero when the
    /// server does not honour the range.
    fn download(&self, url: &str, dest: &Path) -> Result<(), SourceError>;
}

/// Checks a release artifact's signature against the keys configured for its app.
pub trait SignatureVerifier: Send + Sync {
    fn verify(&self, app: &AppId, artifact: &Path, signature: &Path) -> Result<(), VerifyError>;
}

/// Where releases are kept on disk.
pub trait ReleaseStore: Send + Sync {
    /// The working directory of a release being downloaded and unpacked.
    fn partial_dir(&self, app: &AppId, version: &Version) -> PathBuf;
    /// Unpacks `archive` into the partial directory and returns its checked manifest.
    fn unpack(
        &self,
        app: &AppId,
        version: &Version,
        archive: &Path,
    ) -> Result<ReleaseManifest, StoreError>;
    /// Makes an unpacked release kept, and returns its directory.
    fn commit(&self, app: &AppId, version: &Version) -> Result<PathBuf, StoreError>;
    fn release_dir(&self, app: &AppId, version: &Version) -> PathBuf;
    fn manifest(&self, app: &AppId, version: &Version) -> Result<ReleaseManifest, StoreError>;
    /// Deletes a release's partial directory, if any.
    fn discard_partial(&self, app: &AppId, version: &Version);
    fn remove(&self, app: &AppId, version: &Version) -> Result<(), StoreError>;
    /// Deletes every partial directory and returns their paths.
    fn clean_partials(&self) -> Result<Vec<PathBuf>, StoreError>;
}

/// Reads and atomically writes the release state.
pub trait StateStore: Send + Sync {
    fn load(&self) -> Result<ReleaseState, StateError>;
    fn save(&self, state: &ReleaseState) -> Result<(), StateError>;
}

/// A release made ready to go live; what `payload` holds depends on the kind.
pub struct Prepared {
    pub app: AppId,
    pub version: Version,
    pub payload: Box<dyn Any + Send>,
}

/// What an app kind does when one of its releases goes live.
pub trait AppRuntime: Send + Sync {
    /// Loads a release so that activating it is instant. Failing here leaves the current release live.
    fn prepare(
        &self,
        app: &AppId,
        manifest: &ReleaseManifest,
        dir: &Path,
    ) -> Result<Prepared, String>;
    /// Makes a prepared release live, in one step. Cannot fail.
    fn activate(&self, prepared: Prepared);
    /// Lets go of a release that is no longer live.
    fn retire(&self, _app: &AppId, _version: &Version) {}
}
