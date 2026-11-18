//! Minisign (Ed25519) signature checks against the public keys configured per app.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use minisign_verify::{PublicKey, Signature};
use speedyspec_server_domain::{AppId, SignatureVerifier, VerifyError};

/// Accepts an artifact when its signature matches any of its app's configured keys.
pub struct MinisignVerifier {
    keys: BTreeMap<AppId, Vec<PublicKey>>,
}

impl MinisignVerifier {
    /// `keys` maps each app to its base64 public keys, as written in the config. A key that does not decode is an
    /// error naming the app.
    pub fn new(keys: BTreeMap<AppId, Vec<String>>) -> Result<MinisignVerifier, VerifyError> {
        let mut decoded = BTreeMap::new();
        for (app, list) in keys {
            let mut parsed = Vec::with_capacity(list.len());
            for key in &list {
                let key = PublicKey::from_base64(key.trim()).map_err(|err| {
                    VerifyError(format!(
                        "app.{app}.keys: {key:?} is not a minisign public key: {err}"
                    ))
                })?;
                parsed.push(key);
            }
            decoded.insert(app, parsed);
        }
        Ok(MinisignVerifier { keys: decoded })
    }

    fn check(key: &PublicKey, artifact: &Path, signature: &Signature) -> Result<(), String> {
        let mut file = std::fs::File::open(artifact).map_err(|err| err.to_string())?;
        match key.verify_stream(signature) {
            Ok(mut stream) => {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let read = file.read(&mut buf).map_err(|err| err.to_string())?;
                    if read == 0 {
                        break;
                    }
                    stream.update(&buf[..read]);
                }
                stream.finalize().map_err(|err| err.to_string())
            }
            // Legacy (not pre-hashed) signatures cannot be streamed.
            Err(_) => {
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)
                    .map_err(|err| err.to_string())?;
                key.verify(&bytes, signature, false)
                    .map_err(|err| err.to_string())
            }
        }
    }
}

impl SignatureVerifier for MinisignVerifier {
    fn verify(&self, app: &AppId, artifact: &Path, signature: &Path) -> Result<(), VerifyError> {
        let keys = self
            .keys
            .get(app)
            .filter(|keys| !keys.is_empty())
            .ok_or_else(|| {
                VerifyError(format!(
                    "{app}: no signing keys are configured (app.{app}.keys)"
                ))
            })?;
        let text = std::fs::read_to_string(signature)
            .map_err(|err| VerifyError(format!("{app}: reading {}: {err}", signature.display())))?;
        let signature = Signature::decode(&text).map_err(|err| {
            VerifyError(format!(
                "{app}: {} is not a minisign signature: {err}",
                signature.display()
            ))
        })?;
        let mut last = String::new();
        for key in keys {
            match MinisignVerifier::check(key, artifact, &signature) {
                Ok(()) => return Ok(()),
                Err(err) => last = err,
            }
        }

        Err(VerifyError(format!(
            "{app}: the signature of {} does not match any configured key ({last})",
            artifact.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    struct Keys {
        public: String,
        pair: minisign::KeyPair,
    }

    fn keys() -> Keys {
        let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        Keys {
            public: pair.pk.to_base64(),
            pair,
        }
    }

    fn sign(keys: &Keys, data: &[u8]) -> String {
        minisign::sign(
            Some(&keys.pair.pk),
            &keys.pair.sk,
            Cursor::new(data),
            None,
            None,
        )
        .unwrap()
        .into_string()
    }

    fn files(
        data: &[u8],
        signature: &str,
    ) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let (artifact, sig) = (dir.path().join("a.bin"), dir.path().join("a.bin.minisig"));
        std::fs::write(&artifact, data).unwrap();
        std::fs::write(&sig, signature).unwrap();
        (dir, artifact, sig)
    }

    fn verifier(app: &str, public: &[String]) -> MinisignVerifier {
        MinisignVerifier::new(
            [(AppId::parse(app).unwrap(), public.to_vec())]
                .into_iter()
                .collect(),
        )
        .unwrap()
    }

    fn admin() -> AppId {
        AppId::parse("admin").unwrap()
    }

    #[test]
    fn verifies_good_signature() {
        let k = keys();
        let (_d, artifact, sig) = files(b"release bytes", &sign(&k, b"release bytes"));
        verifier("admin", &[k.public.clone()])
            .verify(&admin(), &artifact, &sig)
            .unwrap();
    }

    #[test]
    fn any_configured_key_may_sign() {
        let (old, new) = (keys(), keys());
        let (_d, artifact, sig) = files(b"x", &sign(&new, b"x"));
        verifier("admin", &[old.public.clone(), new.public.clone()])
            .verify(&admin(), &artifact, &sig)
            .unwrap();
    }

    #[test]
    fn rejects_wrong_key() {
        let (signer, other) = (keys(), keys());
        let (_d, artifact, sig) = files(b"x", &sign(&signer, b"x"));
        let err = verifier("admin", &[other.public.clone()])
            .verify(&admin(), &artifact, &sig)
            .unwrap_err();
        assert!(err.0.contains("admin"), "{}", err.0);
    }

    #[test]
    fn rejects_tampered_file() {
        let k = keys();
        let (_d, artifact, sig) = files(b"release bytez", &sign(&k, b"release bytes"));
        assert!(
            verifier("admin", &[k.public.clone()])
                .verify(&admin(), &artifact, &sig)
                .is_err()
        );
    }

    #[test]
    fn rejects_unknown_app() {
        let k = keys();
        let (_d, artifact, sig) = files(b"x", &sign(&k, b"x"));
        let err = verifier("admin", &[k.public.clone()])
            .verify(&AppId::parse("blog").unwrap(), &artifact, &sig)
            .unwrap_err();
        assert!(err.0.contains("blog"), "{}", err.0);
    }

    #[test]
    fn bad_key_rejected_at_construction() {
        let result = MinisignVerifier::new(
            [(admin(), vec!["not a key".to_string()])]
                .into_iter()
                .collect(),
        );
        assert!(result.err().unwrap().0.contains("admin"));
    }
}
