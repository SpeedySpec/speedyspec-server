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
    /// The worker contract majors a runtime binary speaks. Required for kind `runtime`, and only there.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contracts: Vec<u32>,
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
    /// A self-contained HTML file under `root` shown, from memory, by every app in maintenance that names this app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintenance_page: Option<String>,
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
    /// The versions of the runtime this app is configured to run on that this release works with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<semver::VersionReq>,
    /// The app's health probes, which the server calls internally.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthSection>,
}

/// A php app's health probes: paths of the app (starting with `/`) that answer 200 while it is live and ready.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthSection {
    pub live: String,
    pub ready: String,
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
            runtime: None,
            health: None,
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
            (AppKind::Server | AppKind::Php | AppKind::Runtime, Some(_)) => {
                return Err(DomainError::UnexpectedSection {
                    kind,
                    section: "static",
                });
            }
            _ => {}
        }
        match (self.kind, self.contracts.is_empty()) {
            (AppKind::Runtime, true) => {
                return Err(DomainError::MissingSection {
                    kind,
                    section: "contracts",
                });
            }
            (AppKind::Server | AppKind::Static | AppKind::Php, false) => {
                return Err(DomainError::UnexpectedSection {
                    kind,
                    section: "contracts",
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
        if let Some(health) = self.php.as_ref().and_then(|php| php.health.as_ref()) {
            for (field, path) in [
                ("health.live", &health.live),
                ("health.ready", &health.ready),
            ] {
                if !path.starts_with('/') {
                    return Err(DomainError::BadManifest(format!(
                        "php.{field} {path:?} must be a path starting with `/`"
                    )));
                }
            }
        }
        if let Some(page) = self
            .static_
            .as_ref()
            .and_then(|s| s.maintenance_page.as_ref())
            && !inside(page)
        {
            return Err(DomainError::BadManifest(format!(
                "static.maintenance_page {page:?} must be a relative path without `..`"
            )));
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

    #[test]
    fn runtime_manifest_requires_contracts() {
        let json = r#"{"app":"rt","version":"1.0.0","kind":"runtime"}"#;
        assert_eq!(
            ReleaseManifest::parse(json.as_bytes()).unwrap_err(),
            DomainError::MissingSection {
                kind: "runtime",
                section: "contracts"
            }
        );
        let json = r#"{"app":"rt","version":"1.0.0","kind":"runtime","contracts":[1]}"#;
        assert_eq!(
            ReleaseManifest::parse(json.as_bytes()).unwrap().contracts,
            vec![1]
        );
    }

    #[test]
    fn contracts_only_on_runtime() {
        let json = r#"{"app":"blog","version":"1.0.0","kind":"php","contracts":[1]}"#;
        assert_eq!(
            ReleaseManifest::parse(json.as_bytes()).unwrap_err(),
            DomainError::UnexpectedSection {
                kind: "php",
                section: "contracts"
            }
        );
    }

    #[test]
    fn php_section_parses_runtime_requirement() {
        let json = r#"{"app":"blog","version":"1.0.0","kind":"php","php":{"runtime":"^1.4"}}"#;
        let manifest = ReleaseManifest::parse(json.as_bytes()).unwrap();
        assert_eq!(
            manifest.php_section().unwrap().runtime,
            Some(semver::VersionReq::parse("^1.4").unwrap())
        );
    }

    #[test]
    fn health_and_maintenance_page_parse_and_are_checked() {
        let php = r#"{"app":"blog","version":"1.0.0","kind":"php","php":{"health":{"live":"/healthz","ready":"/readyz"}}}"#;
        let health = ReleaseManifest::parse(php.as_bytes())
            .unwrap()
            .php_section()
            .unwrap()
            .health
            .unwrap();
        assert_eq!(
            (health.live.as_str(), health.ready.as_str()),
            ("/healthz", "/readyz")
        );
        let bad = php.replace("\"/healthz\"", "\"healthz\"");
        assert!(ReleaseManifest::parse(bad.as_bytes()).is_err());
        let page = r#"{"app":"admin","version":"1.0.0","kind":"static","static":{"root":"dist","maintenance_page":"maintenance.html"}}"#;
        let manifest = ReleaseManifest::parse(page.as_bytes()).unwrap();
        assert_eq!(
            manifest.static_.unwrap().maintenance_page.as_deref(),
            Some("maintenance.html")
        );
        assert!(
            ReleaseManifest::parse(page.replace("maintenance.html", "../x.html").as_bytes())
                .is_err()
        );
    }
}
