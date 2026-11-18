//! A release index: the JSON an app's `source` URL serves, listing its releases and their artifacts.

use std::collections::BTreeSet;

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{AppId, DomainError};

/// Every release of one app that its source offers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseIndex {
    pub app: AppId,
    #[serde(default)]
    pub releases: Vec<IndexEntry>,
}

/// One release in an index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub version: Version,
    #[serde(default)]
    pub published: String,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
}

/// A downloadable file of a release, for one target (`any` for static and php apps, a Rust target triple for the
/// server).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub target: String,
    pub url: String,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: String,
    /// URL of the file's minisign signature.
    #[serde(default)]
    pub minisig: Option<String>,
}

impl ReleaseIndex {
    /// Parses an index and refuses duplicate versions, build metadata (which SemVer does not order) and malformed
    /// checksums.
    pub fn parse(bytes: &[u8]) -> Result<ReleaseIndex, DomainError> {
        let index: ReleaseIndex =
            serde_json::from_slice(bytes).map_err(|err| DomainError::BadIndex(err.to_string()))?;
        let mut seen = BTreeSet::new();
        for entry in &index.releases {
            if !entry.version.build.is_empty() {
                return Err(DomainError::BadIndex(format!(
                    "version {} has build metadata, which has no order",
                    entry.version
                )));
            }
            if !seen.insert(&entry.version) {
                return Err(DomainError::BadIndex(format!(
                    "version {} is listed twice",
                    entry.version
                )));
            }
            for artifact in &entry.artifacts {
                let hex = artifact.sha256.len() == 64
                    && artifact
                        .sha256
                        .chars()
                        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
                if !hex {
                    return Err(DomainError::BadIndex(format!(
                        "version {}: sha256 {:?} is not 64 lowercase hex characters",
                        entry.version, artifact.sha256
                    )));
                }
            }
        }

        Ok(index)
    }

    /// The highest version strictly newer than `after`, skipping pre-releases unless `prerelease` is set.
    pub fn newest(&self, after: Option<&Version>, prerelease: bool) -> Option<&IndexEntry> {
        self.releases
            .iter()
            .filter(|entry| prerelease || entry.version.pre.is_empty())
            .filter(|entry| after.is_none_or(|after| entry.version > *after))
            .max_by(|a, b| a.version.cmp(&b.version))
    }

    pub fn entry(&self, version: &Version) -> Option<&IndexEntry> {
        self.releases.iter().find(|entry| entry.version == *version)
    }
}

impl IndexEntry {
    pub fn artifact_for(&self, target: &str) -> Option<&Artifact> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.target == target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC_EXAMPLE: &str = r#"{
        "app": "admin",
        "releases": [
            {
                "version": "5.2.0",
                "published": "2026-11-17T12:00:00Z",
                "artifacts": [
                    { "target": "any", "url": "https://x/admin-5.2.0.tar.zst",
                      "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
                      "minisig": "https://x/admin-5.2.0.tar.zst.minisig" }
                ]
            }
        ]
    }"#;

    fn index(versions: &[&str]) -> ReleaseIndex {
        let releases: Vec<String> = versions
            .iter()
            .map(|v| format!(r#"{{"version":"{v}","published":"x","artifacts":[]}}"#))
            .collect();
        ReleaseIndex::parse(
            format!(r#"{{"app":"admin","releases":[{}]}}"#, releases.join(",")).as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn index_parses_spec_example() {
        let index = ReleaseIndex::parse(SPEC_EXAMPLE.as_bytes()).unwrap();
        assert_eq!(index.app.as_str(), "admin");
        assert_eq!(index.releases[0].version, Version::new(5, 2, 0));
        assert!(
            index.releases[0]
                .artifact_for("any")
                .unwrap()
                .minisig
                .is_some()
        );
        assert!(
            index.releases[0]
                .artifact_for("x86_64-unknown-linux-musl")
                .is_none()
        );
    }

    #[test]
    fn index_rejects() {
        let bad = [
            r#"{"app":"admin","releases":[{"version":"1.0.0","published":"x","artifacts":[]},{"version":"1.0.0","published":"y","artifacts":[]}]}"#,
            r#"{"app":"admin","releases":[{"version":"1.0.0+abc","published":"x","artifacts":[]}]}"#,
            r#"{"app":"Admin","releases":[]}"#,
            r#"{"app":"admin","releases":[{"version":"1.0.0","published":"x","artifacts":[{"target":"any","url":"u","sha256":"ABC"}]}]}"#,
        ];
        for json in bad {
            assert!(
                ReleaseIndex::parse(json.as_bytes()).is_err(),
                "must be rejected: {json}"
            );
        }
    }

    #[test]
    fn newest_respects_prerelease() {
        let index = index(&["1.0.0", "1.1.0-rc.1", "1.0.1"]);
        let v = |s: &str| Version::parse(s).unwrap();
        assert_eq!(
            index.newest(Some(&v("1.0.0")), false).unwrap().version,
            v("1.0.1")
        );
        assert_eq!(
            index.newest(Some(&v("1.0.1")), true).unwrap().version,
            v("1.1.0-rc.1")
        );
        assert!(index.newest(Some(&v("1.0.1")), false).is_none());
        assert_eq!(index.newest(None, false).unwrap().version, v("1.0.1"));
    }
}
