//! The PHP runtime binary, for tests that serve PHP apps.
//!
//! The server starts `speedyspec-php-runtime` from next to its own executable. A test of this crate cannot depend
//! on another crate's binary, so this builds it once per test process (in its own target directory, so the build
//! never waits on the `cargo test` that runs us) and copies it next to the server binary under test.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The runtime binary next to the server binary, built and copied on first use.
pub fn binary() -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(build).clone()
}

fn build() -> PathBuf {
    let server = Path::new(super::run::BIN);
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let target_dir = server
        .ancestors()
        .find(|dir| {
            dir.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("target"))
        })
        .expect("the server binary is under a target directory")
        .join("runtime-for-tests");
    let output = Command::new(env!("CARGO"))
        .args([
            "build",
            "-p",
            "speedyspec-php-runtime",
            "--message-format=json-render-diagnostics",
        ])
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target_dir)
        .output()
        .expect("running cargo");
    assert!(
        output.status.success(),
        "building speedyspec-php-runtime failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let built = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| message["target"]["name"] == "speedyspec-php-runtime")
        .find_map(|message| message["executable"].as_str().map(PathBuf::from))
        .expect("cargo reported the runtime executable");
    let next_to_server = server.with_file_name("speedyspec-php-runtime");
    if std::fs::read(&next_to_server).ok() != std::fs::read(&built).ok() {
        let staged =
            server.with_file_name(format!("speedyspec-php-runtime.{}", std::process::id()));
        std::fs::copy(&built, &staged).expect("copying the runtime binary");
        std::fs::rename(&staged, &next_to_server).expect("placing the runtime binary");
    }
    // macOS scans a binary the first time it runs, which would delay a test's first worker by a second or more: run
    // it once here, before any test measures time.
    let warmed = Command::new(&next_to_server)
        .arg("manifest")
        .output()
        .expect("running the runtime binary");
    assert!(
        warmed.status.success(),
        "the runtime binary's manifest failed"
    );
    next_to_server
}
