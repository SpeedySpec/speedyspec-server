//! A signature verifier that accepts a signature file reading exactly `good`.

use std::path::Path;

use speedyspec_server_domain::{AppId, SignatureVerifier, VerifyError};

pub struct MemoryVerifier;

impl SignatureVerifier for MemoryVerifier {
    fn verify(&self, app: &AppId, _artifact: &Path, signature: &Path) -> Result<(), VerifyError> {
        match std::fs::read(signature) {
            Ok(bytes) if bytes == b"good" => Ok(()),
            _ => Err(VerifyError(format!(
                "{app}: signature does not match any configured key"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_good() {
        let dir = std::env::temp_dir().join(format!("memver-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (good, bad) = (dir.join("good"), dir.join("bad"));
        std::fs::write(&good, "good").unwrap();
        std::fs::write(&bad, "bad").unwrap();
        let app = AppId::parse("admin").unwrap();
        assert!(MemoryVerifier.verify(&app, &good, &good).is_ok());
        assert!(MemoryVerifier.verify(&app, &good, &bad).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
