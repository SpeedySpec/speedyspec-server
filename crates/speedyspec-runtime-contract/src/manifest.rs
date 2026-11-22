//! What `speedyspec-php-runtime manifest` prints.

use serde::{Deserialize, Serialize};

/// A runtime binary's description of itself.
///
/// Fields are added within contract 1 (old readers ignore them), so the struct is `#[non_exhaustive]`: outside this
/// crate a manifest is made with [`RuntimeManifest::new`] and the `with_` methods, or deserialized, never written as
/// a struct literal.
///
/// ```compile_fail
/// let manifest = speedyspec_runtime_contract::RuntimeManifest {
///     version: "1.2.3".parse().unwrap(),
///     contracts: vec![1],
///     php: "8.5.12".into(),
///     sodium: None,
///     ffmpeg: None,
/// };
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RuntimeManifest {
    /// The runtime's own version.
    pub version: semver::Version,
    /// The contract majors it speaks.
    pub contracts: Vec<u32>,
    /// The embedded PHP's version.
    pub php: String,
    /// The libsodium version ext-sodium and the Rust crypto port share. Absent from runtimes released before it was
    /// added, so it stays optional within contract 1; the release smoke check requires it of new releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sodium: Option<String>,
    /// The FFmpeg release the runtime reads media metadata with, linked statically under LGPL 2.1 (each release attaches
    /// its source). Optional within contract 1, like `sodium`; the release smoke check requires it of new releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ffmpeg: Option<String>,
}

impl RuntimeManifest {
    /// A manifest with the fields every runtime reports; the optional ones are absent until given.
    pub fn new(version: semver::Version, contracts: Vec<u32>, php: impl Into<String>) -> Self {
        RuntimeManifest {
            version,
            contracts,
            php: php.into(),
            sodium: None,
            ffmpeg: None,
        }
    }

    /// The manifest, reporting libsodium `version`.
    pub fn with_sodium(mut self, version: impl Into<String>) -> Self {
        self.sodium = Some(version.into());
        self
    }

    /// The manifest, reporting FFmpeg `version`.
    pub fn with_ffmpeg(mut self, version: impl Into<String>) -> Self {
        self.ffmpeg = Some(version.into());
        self
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_without_sodium_parses() {
        // What runtimes released before the sodium field print.
        let json = r#"{"version":"1.0.4","contracts":[1],"php":"8.5.12"}"#;
        let manifest: RuntimeManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.sodium, None);
    }

    #[test]
    fn manifest_with_sodium_round_trips() {
        let manifest = RuntimeManifest {
            version: "1.2.3".parse().unwrap(),
            contracts: vec![1],
            php: "8.5.12".into(),
            sodium: Some("1.0.22".into()),
            ffmpeg: None,
        };
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains(r#""sodium":"1.0.22""#), "{json}");
        assert_eq!(
            serde_json::from_str::<RuntimeManifest>(&json).unwrap(),
            manifest
        );
    }

    #[test]
    fn manifest_without_ffmpeg_still_parses() {
        // What runtimes released before the ffmpeg field print.
        let json = r#"{"version":"1.0.4","contracts":[1],"php":"8.5.12","sodium":"1.0.22"}"#;
        let manifest: RuntimeManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.ffmpeg, None);
    }

    #[test]
    fn manifest_round_trips_ffmpeg() {
        let manifest =
            RuntimeManifest::new("1.2.3".parse().unwrap(), vec![1], "8.5.12").with_ffmpeg("9.1");
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains(r#""ffmpeg":"9.1""#), "{json}");
        assert_eq!(
            serde_json::from_str::<RuntimeManifest>(&json).unwrap(),
            manifest
        );
    }

    #[test]
    fn manifest_round_trips() {
        let manifest = RuntimeManifest {
            version: "1.2.3".parse().unwrap(),
            contracts: vec![1],
            php: "8.5.12".into(),
            sodium: None,
            ffmpeg: None,
        };
        let json = serde_json::to_string(&manifest).unwrap();
        assert_eq!(
            serde_json::from_str::<RuntimeManifest>(&json).unwrap(),
            manifest
        );
    }
}
