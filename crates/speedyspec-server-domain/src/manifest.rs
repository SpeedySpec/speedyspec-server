//! The release manifest, `speedyspec-release.json`, at the root of every release.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use crate::{AppId, AppKind, DomainError};

/// The file name of the manifest at a release's root.
pub const MANIFEST_FILE: &str = "speedyspec-release.json";

/// What a release says about itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    pub app: AppId,
    pub version: semver::Version,
    pub kind: AppKind,
    /// Other apps' version ranges this release works with.
    #[serde(default)]
    pub requires: BTreeMap<AppId, semver::VersionReq>,
    #[serde(rename = "static", default, skip_serializing_if = "Option::is_none")]
    pub static_: Option<StaticSection>,
    /// Defined by the PHP worker project; stored unread here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub php: Option<serde_json::Value>,
}

/// How a static release is served.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaticSection {
    /// The directory, relative to the release root, whose files are served.
    pub root: String,
    /// A file under `root` served with status 404 when nothing matches.
    #[serde(default)]
    pub not_found: Option<String>,
    /// `Cache-Control` rules; the first match wins.
    #[serde(default)]
    pub cache: Vec<CacheRule>,
}

/// A `Cache-Control` value for the files a glob matches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheRule {
    /// A glob relative to `root`, with `/` separators.
    #[serde(rename = "match")]
    pub pattern: String,
    pub cache_control: String,
}

impl ReleaseManifest {
    /// Parses and checks a manifest's JSON.
    pub fn parse(bytes: &[u8]) -> Result<ReleaseManifest, DomainError> {
        let manifest: ReleaseManifest = serde_json::from_slice(bytes)
            .map_err(|err| DomainError::BadManifest(err.to_string()))?;
        manifest.check()?;

        Ok(manifest)
    }

    fn check(&self) -> Result<(), DomainError> {
        let kind = self.kind.as_str();
        match (self.kind, &self.static_) {
            (AppKind::Static, None) => {
                return Err(DomainError::MissingSection {
                    kind,
                    section: "static",
                });
            }
            (AppKind::Server | AppKind::Php, Some(_)) => {
                return Err(DomainError::UnexpectedSection {
                    kind,
                    section: "static",
                });
            }
            _ => {}
        }
        if self.kind != AppKind::Php && self.php.is_some() {
            return Err(DomainError::UnexpectedSection {
                kind,
                section: "php",
            });
        }
        if let Some(section) = &self.static_ {
            let root = Path::new(&section.root);
            let relative = root
                .components()
                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
            if section.root.is_empty() || !relative {
                return Err(DomainError::BadStaticRoot(section.root.clone()));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC_EXAMPLE: &str = r#"{
        "app": "admin",
        "version": "5.2.0",
        "kind": "static",
        "requires": { "server": "^3.1" },
        "static": {
            "root": "dist",
            "not_found": "404.html",
            "cache": [
                { "match": "_astro/**", "cache_control": "public, max-age=31536000, immutable" },
                { "match": "**/*.html", "cache_control": "no-cache" }
            ]
        }
    }"#;

    #[test]
    fn manifest_parses_spec_example() {
        let manifest = ReleaseManifest::parse(SPEC_EXAMPLE.as_bytes()).unwrap();
        assert_eq!(manifest.app.as_str(), "admin");
        assert_eq!(manifest.version, semver::Version::new(5, 2, 0));
        assert_eq!(manifest.kind, AppKind::Static);
        let server = AppId::parse("server").unwrap();
        assert_eq!(
            manifest.requires[&server],
            semver::VersionReq::parse("^3.1").unwrap()
        );
        let section = manifest.static_.as_ref().unwrap();
        assert_eq!(section.root, "dist");
        assert_eq!(section.not_found.as_deref(), Some("404.html"));
        assert_eq!(section.cache[0].pattern, "_astro/**");
        assert_eq!(
            section.cache[0].cache_control,
            "public, max-age=31536000, immutable"
        );
        assert!(manifest.php.is_none());
    }

    fn rejects(json: &str) {
        assert!(
            ReleaseManifest::parse(json.as_bytes()).is_err(),
            "must be rejected: {json}"
        );
    }

    #[test]
    fn manifest_rejects() {
        rejects(r#"{"app":"admin","kind":"static","static":{"root":"dist"}}"#);
        rejects(r#"{"app":"admin","version":"five","kind":"static","static":{"root":"dist"}}"#);
        rejects(
            r#"{"app":"admin","version":"1.0.0","kind":"static","requires":{"Bad":"^1"},"static":{"root":"dist"}}"#,
        );
        rejects(r#"{"app":"admin","version":"1.0.0","kind":"static"}"#);
        rejects(r#"{"app":"blog","version":"1.0.0","kind":"php","static":{"root":"dist"}}"#);
        rejects(r#"{"app":"admin","version":"1.0.0","kind":"static","static":{"root":"/dist"}}"#);
        rejects(
            r#"{"app":"admin","version":"1.0.0","kind":"static","static":{"root":"a/../../dist"}}"#,
        );
    }

    #[test]
    fn php_section_is_kept_unread() {
        let manifest = ReleaseManifest::parse(
            br#"{"app":"blog","version":"1.0.0","kind":"php","php":{"entry":"index.php"}}"#,
        )
        .unwrap();
        assert_eq!(manifest.php.unwrap()["entry"], "index.php");
        assert!(manifest.requires.is_empty());
    }
}
