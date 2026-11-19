//! Releases on disk under `<data_dir>/releases/<app>/`.

use std::path::{Path, PathBuf};

use semver::Version;
use speedyspec_server_domain::{AppId, MANIFEST_FILE, ReleaseManifest, ReleaseStore, StoreError};

use crate::unpack::{MAX_UNPACKED_BYTES, unpack};

/// Committed releases live in `releases/<app>/<version>/`. A release being installed lives in
/// `releases/<app>/<version>.partial/` (the downloaded `artifact`, its `artifact.minisig`, and `unpacked/`); `commit`
/// renames `unpacked/` into place and deletes the rest.
pub struct FsReleaseStore {
    releases: PathBuf,
    limit: u64,
}

fn error(path: &Path, message: impl ToString) -> StoreError {
    StoreError {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

impl FsReleaseStore {
    pub fn new(data_dir: &Path) -> FsReleaseStore {
        FsReleaseStore::with_limit(data_dir, MAX_UNPACKED_BYTES)
    }

    /// A store whose releases may unpack to at most `limit` bytes. For tests.
    #[doc(hidden)]
    pub fn with_limit(data_dir: &Path, limit: u64) -> FsReleaseStore {
        FsReleaseStore {
            releases: data_dir.join("releases"),
            limit,
        }
    }

    fn read_manifest(dir: &Path) -> Result<ReleaseManifest, StoreError> {
        let path = dir.join(MANIFEST_FILE);
        let bytes = std::fs::read(&path).map_err(|err| error(&path, err))?;
        ReleaseManifest::parse(&bytes).map_err(|err| error(&path, err))
    }
}

impl ReleaseStore for FsReleaseStore {
    fn partial_dir(&self, app: &AppId, version: &Version) -> PathBuf {
        self.releases
            .join(app.as_str())
            .join(format!("{version}.partial"))
    }

    fn unpack(
        &self,
        app: &AppId,
        version: &Version,
        archive: &Path,
    ) -> Result<ReleaseManifest, StoreError> {
        let partial = self.partial_dir(app, version);
        let dest = partial.join("unpacked");
        let result = (|| {
            if dest.exists() {
                std::fs::remove_dir_all(&dest).map_err(|err| error(&dest, err))?;
            }
            std::fs::create_dir_all(&dest).map_err(|err| error(&dest, err))?;
            unpack(archive, &dest, self.limit).map_err(|message| error(archive, message))?;
            let manifest = FsReleaseStore::read_manifest(&dest)?;
            if manifest.app != *app {
                return Err(error(
                    &dest.join(MANIFEST_FILE),
                    format!("app is {}, expected {app}", manifest.app),
                ));
            }
            if manifest.version != *version {
                return Err(error(
                    &dest.join(MANIFEST_FILE),
                    format!("version is {}, expected {version}", manifest.version),
                ));
            }
            if let Some(php) = manifest.php_section() {
                let entry = Path::new(&php.document_root).join(&php.entry);
                if !dest.join(&entry).is_file() {
                    return Err(error(
                        &dest.join(MANIFEST_FILE),
                        format!("the php entry {} is not in the release", entry.display()),
                    ));
                }
            }
            Ok(manifest)
        })();
        if result.is_err() {
            self.discard_partial(app, version);
        }
        result
    }

    fn commit(&self, app: &AppId, version: &Version) -> Result<PathBuf, StoreError> {
        let partial = self.partial_dir(app, version);
        let target = self.release_dir(app, version);
        if target.exists() {
            std::fs::remove_dir_all(&target).map_err(|err| error(&target, err))?;
        }
        std::fs::rename(partial.join("unpacked"), &target).map_err(|err| error(&target, err))?;
        std::fs::remove_dir_all(&partial).map_err(|err| error(&partial, err))?;
        Ok(target)
    }

    fn commit_binary(
        &self,
        app: &AppId,
        version: &Version,
        artifact: &Path,
    ) -> Result<PathBuf, StoreError> {
        use std::os::unix::fs::PermissionsExt;

        let dir = self.release_dir(app, version);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|err| error(&dir, err))?;
        }
        std::fs::create_dir_all(&dir).map_err(|err| error(&dir, err))?;
        let path = dir.join("speedyspec-server");
        std::fs::rename(artifact, &path).map_err(|err| error(&path, err))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .map_err(|err| error(&path, err))?;
        self.discard_partial(app, version);
        Ok(path)
    }

    fn release_dir(&self, app: &AppId, version: &Version) -> PathBuf {
        self.releases.join(app.as_str()).join(version.to_string())
    }

    fn manifest(&self, app: &AppId, version: &Version) -> Result<ReleaseManifest, StoreError> {
        FsReleaseStore::read_manifest(&self.release_dir(app, version))
    }

    fn discard_partial(&self, app: &AppId, version: &Version) {
        let partial = self.partial_dir(app, version);
        if partial.exists()
            && let Err(err) = std::fs::remove_dir_all(&partial)
        {
            tracing::warn!(path = %partial.display(), error = %err, "could not remove a partial release");
        }
    }

    fn remove(&self, app: &AppId, version: &Version) -> Result<(), StoreError> {
        let dir = self.release_dir(app, version);
        std::fs::remove_dir_all(&dir).map_err(|err| error(&dir, err))
    }

    fn clean_partials(&self) -> Result<Vec<PathBuf>, StoreError> {
        let mut cleaned = Vec::new();
        let Ok(apps) = std::fs::read_dir(&self.releases) else {
            return Ok(cleaned);
        };
        for app in apps.flatten() {
            let Ok(entries) = std::fs::read_dir(app.path()) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "partial") {
                    std::fs::remove_dir_all(&path).map_err(|err| error(&path, err))?;
                    cleaned.push(path);
                }
            }
        }
        cleaned.sort();
        Ok(cleaned)
    }
}
