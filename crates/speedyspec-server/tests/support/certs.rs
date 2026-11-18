//! Test certificates: a throwaway CA and a leaf for `localhost` / `127.0.0.1` signed by it.

use std::path::{Path, PathBuf};

use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use speedyspec_server::listeners::TlsFiles;

/// The leaf certificate files the server uses, and the CA a client must trust.
pub struct TestCerts {
    pub tls: TlsFiles,
    pub ca: PathBuf,
}

/// Writes `ca.pem`, `cert.pem` and `key.pem` into `dir`.
pub fn generate(dir: &Path) -> TestCerts {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca_params, ca_key);

    let leaf_key = KeyPair::generate().unwrap();
    let leaf_params =
        CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()]).unwrap();
    let leaf_cert = leaf_params.signed_by(&leaf_key, &issuer).unwrap();

    let ca = dir.join("ca.pem");
    let cert = dir.join("cert.pem");
    let key = dir.join("key.pem");
    std::fs::write(&ca, ca_cert.pem()).unwrap();
    std::fs::write(&cert, leaf_cert.pem()).unwrap();
    std::fs::write(&key, leaf_key.serialize_pem()).unwrap();

    TestCerts {
        tls: TlsFiles { cert, key },
        ca,
    }
}
