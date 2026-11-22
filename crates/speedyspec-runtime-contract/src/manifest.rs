//! What `speedyspec-php-runtime manifest` prints.

use serde::{Deserialize, Serialize};

/// A runtime binary's description of itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeManifest {
    pub version: semver::Version,
    /// The contract majors it speaks.
    pub contracts: Vec<u32>,
    /// The embedded PHP's version.
    pub php: String,
    /// The libsodium version ext-sodium and the Rust crypto port share. Absent from runtimes released before it was
    /// added, so it stays optional within contract 1; the release smoke check requires it of new releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sodium: Option<String>,
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
        };
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains(r#""sodium":"1.0.22""#), "{json}");
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
        };
        let json = serde_json::to_string(&manifest).unwrap();
        assert_eq!(
            serde_json::from_str::<RuntimeManifest>(&json).unwrap(),
            manifest
        );
    }
}
