//! How a worker is started: `speedyspec-php-runtime worker --spec <json>`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What a worker is started with, as JSON. Unknown fields are ignored (a newer server may add some).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerSpec {
    /// The contract major the server speaks.
    pub contract: u32,
    pub app: String,
    /// The worker's FastCGI socket.
    pub socket: PathBuf,
    /// The release's root.
    pub release_dir: PathBuf,
    pub site_dir: Option<PathBuf>,
    /// The release's `php.ini` settings.
    pub ini: BTreeMap<String, String>,
    /// `app.<id>.php.bindings`: `(port key, PHP class)`.
    pub bindings: Vec<(String, String)>,
    /// The app's environment, set before PHP starts (the server merges the site's `.env`, `env_file` and `env`).
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

impl WorkerSpec {
    /// Refuses a spec of a contract major this build does not speak.
    pub fn check(&self) -> Result<(), String> {
        if self.contract == crate::CONTRACT_MAJOR {
            Ok(())
        } else {
            Err(format!(
                "the server speaks worker contract {}, this runtime speaks contract {}",
                self.contract,
                crate::CONTRACT_MAJOR
            ))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> WorkerSpec {
        WorkerSpec {
            contract: crate::CONTRACT_MAJOR,
            app: "blog".into(),
            socket: PathBuf::from("/run/s.sock"),
            release_dir: PathBuf::from("/r"),
            site_dir: None,
            ini: BTreeMap::new(),
            bindings: Vec::new(),
            env: vec![("A".into(), "1".into())],
        }
    }

    #[test]
    fn this_major_is_accepted() {
        assert_eq!(sample().check(), Ok(()));
    }

    #[test]
    fn a_spec_of_another_major_is_refused_naming_both() {
        let spec = WorkerSpec {
            contract: 2,
            ..sample()
        };
        let err = spec.check().unwrap_err();
        assert!(
            err.contains("contract 2") && err.contains("contract 1"),
            "{err}"
        );
    }

    #[test]
    fn a_spec_without_contract_does_not_parse() {
        let json = r#"{"app":"a","socket":"/s","release_dir":"/r","site_dir":null,"ini":{},"bindings":[],"env":[]}"#;
        assert!(serde_json::from_str::<WorkerSpec>(json).is_err());
    }

    #[test]
    fn unknown_spec_fields_are_ignored() {
        // Adding a field is a minor contract change: the other side ignores it.
        let mut value = serde_json::to_value(sample()).unwrap();
        value["later_field"] = serde_json::json!(true);
        assert_eq!(
            serde_json::from_value::<WorkerSpec>(value).unwrap(),
            sample()
        );
    }
}
