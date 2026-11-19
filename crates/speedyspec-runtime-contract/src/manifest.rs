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
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trips() {
        let manifest = RuntimeManifest {
            version: "1.2.3".parse().unwrap(),
            contracts: vec![1],
            php: "8.5.12".into(),
        };
        let json = serde_json::to_string(&manifest).unwrap();
        assert_eq!(
            serde_json::from_str::<RuntimeManifest>(&json).unwrap(),
            manifest
        );
    }
}
