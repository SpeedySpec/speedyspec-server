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
    /// How a php release is served; a php release without one gets the defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub php: Option<PhpSection>,
    /// The keeper protocol a server binary speaks. Only for kind `server`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<u32>,
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

/// How a php release is served.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhpSection {
    /// The directory, relative to the release root, that URLs map into.
    #[serde(default = "PhpSection::default_document_root")]
    pub document_root: String,
    /// The front controller under `document_root`, which gets every path that is not an existing file.
    #[serde(default = "PhpSection::default_entry")]
    pub entry: String,
    /// INI settings for the release's workers.
    #[serde(default)]
    pub ini: BTreeMap<String, String>,
}

impl PhpSection {
    fn default_document_root() -> String {
        ".".to_string()
    }

    fn default_entry() -> String {
        "index.php".to_string()
    }
}

impl Default for PhpSection {
    fn default() -> Self {
        PhpSection {
            document_root: Self::default_document_root(),
            entry: Self::default_entry(),
            ini: BTreeMap::new(),
        }
    }
}

/// A relative path with no `..`, absolute or empty part.
fn inside(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
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

    /// The php section of a php release (the defaults when it has none); `None` for other kinds.
    pub fn php_section(&self) -> Option<PhpSection> {
        (self.kind == AppKind::Php).then(|| self.php.clone().unwrap_or_default())
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
        if self.kind != AppKind::Server && self.protocol.is_some() {
            return Err(DomainError::UnexpectedSection {
                kind,
                section: "protocol",
            });
        }
        if self.kind != AppKind::Php && self.php.is_some() {
            return Err(DomainError::UnexpectedSection {
                kind,
                section: "php",
            });
        }
        if let Some(php) = &self.php {
            for (field, value) in [("document_root", &php.document_root), ("entry", &php.entry)] {
                if !inside(value) {
                    return Err(DomainError::BadPhpPath {
                        field,
                        path: value.clone(),
                    });
                }
            }
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
    fn php_section_parses_with_defaults() {
        let manifest = ReleaseManifest::parse(
            br#"{"app":"blog","version":"1.0.0","kind":"php","php":{"entry":"index.php","ini":{"memory_limit":"64M"}}}"#,
        )
        .unwrap();
        let php = manifest.php_section().unwrap();
        assert_eq!(
            (php.document_root.as_str(), php.entry.as_str()),
            (".", "index.php")
        );
        assert_eq!(php.ini["memory_limit"], "64M");
        let bare =
            ReleaseManifest::parse(br#"{"app":"blog","version":"1.0.0","kind":"php"}"#).unwrap();
        let php = bare.php_section().unwrap();
        assert_eq!(
            (php.document_root.as_str(), php.entry.as_str()),
            (".", "index.php")
        );
        assert!(manifest.requires.is_empty());
        let static_ = ReleaseManifest::parse(SPEC_EXAMPLE.as_bytes()).unwrap();
        assert!(
            static_.php_section().is_none(),
            "only php releases have one"
        );
    }

    #[test]
    fn php_section_paths_stay_inside_the_release() {
        for bad in [
            r#"{"document_root":"../up"}"#,
            r#"{"entry":"/etc/passwd"}"#,
            r#"{"entry":"a/../../b.php"}"#,
            r#"{"entry":""}"#,
        ] {
            let json = format!(r#"{{"app":"blog","version":"1.0.0","kind":"php","php":{bad}}}"#);
            let err = ReleaseManifest::parse(json.as_bytes())
                .unwrap_err()
                .to_string();
            assert!(err.contains("php"), "{bad}: {err}");
        }
        let unknown = br#"{"app":"blog","version":"1.0.0","kind":"php","php":{"entri":"x.php"}}"#;
        assert!(
            ReleaseManifest::parse(unknown).is_err(),
            "unknown fields are refused"
        );
    }

    #[test]
    fn protocol_only_on_server_manifests() {
        let server = ReleaseManifest::parse(
            br#"{"app":"server","version":"0.2.0","kind":"server","protocol":1}"#,
        )
        .unwrap();
        assert_eq!(server.protocol, Some(1));
        assert!(ReleaseManifest::parse(br#"{"app":"admin","version":"1.0.0","kind":"static","static":{"root":"d"},"protocol":1}"#).is_err());
        let json = serde_json::to_string(
            &ReleaseManifest::parse(MANIFEST_NO_PROTOCOL.as_bytes()).unwrap(),
        )
        .unwrap();
        assert!(!json.contains("protocol"), "{json}");
    }

    const MANIFEST_NO_PROTOCOL: &str = r#"{"app":"server","version":"0.1.0","kind":"server"}"#;
}
