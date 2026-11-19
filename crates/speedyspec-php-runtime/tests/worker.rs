//! The runtime binary's own commands.

use std::process::Command;

use speedyspec_runtime_contract::RuntimeManifest;

const BIN: &str = env!("CARGO_BIN_EXE_speedyspec-php-runtime");

#[test]
fn manifest_prints_contract_majors() {
    let output = Command::new(BIN).arg("manifest").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest: RuntimeManifest = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(manifest.contracts, vec![1]);
    assert_eq!(manifest.version.to_string(), env!("CARGO_PKG_VERSION"));
    assert!(manifest.php.starts_with("8.5."), "{}", manifest.php);
}

#[test]
fn worker_refuses_another_major() {
    let spec = serde_json::json!({
        "contract": 9, "app": "a", "socket": "/nonexistent/s.sock", "release_dir": "/", "site_dir": null,
        "ini": {}, "bindings": [], "env": []
    });
    let output = Command::new(BIN)
        .args(["worker", "--spec", &spec.to_string()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("contract 9"));
}
