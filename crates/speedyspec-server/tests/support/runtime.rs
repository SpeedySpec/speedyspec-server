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

/// How a test runtime release's workers behave.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Workers {
    /// Run the real runtime binary, with `SPEEDYSPEC_RUNTIME_LABEL` set to the release's version.
    Real,
    /// Exit at once, so no pool can start.
    ExitAtOnce,
}

/// A runtime release directory `<parent>/<version>/` whose `speedyspec-php-runtime` is a wrapper script: `manifest`
/// prints this version's manifest, and `worker` runs the real binary (or exits, for [`Workers::ExitAtOnce`]).
pub fn release(parent: &Path, version: &str, workers: Workers) -> PathBuf {
    let dir = parent.join(version);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("speedyspec-php-runtime"), script(version, workers)).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        dir.join("speedyspec-php-runtime"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    dir
}

/// The wrapper script's text (what a managed test publishes as the release's binary).
pub fn script(version: &str, workers: Workers) -> String {
    let worker = match workers {
        Workers::Real => format!(
            "export SPEEDYSPEC_RUNTIME_LABEL={version}\nexec '{}' \"$@\"",
            binary().display()
        ),
        Workers::ExitAtOnce => "exit 1".to_string(),
    };
    format!(
        "#!/bin/sh\nif [ \"$1\" = manifest ]; then\n  echo '{{\"version\":\"{version}\",\"contracts\":[1],\"php\":\"test\"}}'\n  exit 0\nfi\n{worker}\n"
    )
}
