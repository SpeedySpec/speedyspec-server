//! `scripts/build-php.sh`'s pinned downloads (`scripts/lib/pinned-downloads.sh`), run against `file://` fixtures: the
//! checksum check before anything is used, and the check that spc built from the pinned libsodium.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// libsodium 1.0.22's tarball checksum, as `scripts/build-php.sh` pins it.
const LIBSODIUM_SHA256: &str = "adbdd8f16149e81ac6078a03aca6fc03b592b89ef7b5ed83841c086191be3349";

fn helpers() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/lib/pinned-downloads.sh")
}

/// Sources the helpers and runs `call` (a shell command line) with `args` as `$1`, `$2`, … under `set -euo pipefail`.
fn run(call: &str, args: &[&str]) -> Output {
    Command::new("bash")
        .arg("-c")
        .arg(format!("set -euo pipefail; source \"$0\"; {call}"))
        .arg(helpers())
        .args(args)
        .output()
        .unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn sha256(path: &Path) -> String {
    let output = run("sha256_of \"$1\"", &[path.to_str().unwrap()]);
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn a_libsodium_tarball_with_another_checksum_is_refused_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let served = dir.path().join("served.tar.gz");
    std::fs::write(&served, b"not libsodium").unwrap();
    let dest = dir.path().join("pinned/libsodium-1.0.22.tar.gz");
    let url = format!("file://{}", served.display());
    let output = run(
        "fetch_pinned \"$1\" \"$2\" \"$3\" \"$4\"",
        &[
            &url,
            LIBSODIUM_SHA256,
            dest.to_str().unwrap(),
            "libsodium-1.0.22.tar.gz",
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let err = stderr(&output);
    assert!(
        err.contains("checksum mismatch for libsodium-1.0.22.tar.gz"),
        "{err}"
    );
    assert!(
        err.contains(&format!("expected {LIBSODIUM_SHA256}")),
        "{err}"
    );
    assert!(
        err.contains(&format!("actual   {}", sha256(&served))),
        "{err}"
    );
    assert!(!dest.exists(), "a refused download must not be kept");
    assert!(
        !dest.with_extension("gz.download").exists(),
        "the partial download must be removed"
    );
}

#[test]
fn a_tarball_with_the_pinned_checksum_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let served = dir.path().join("served.tar.gz");
    std::fs::write(&served, b"pinned bytes").unwrap();
    let pinned = sha256(&served);
    let dest = dir.path().join("pinned/x.tar.gz");
    let url = format!("file://{}", served.display());
    let output = run(
        "fetch_pinned \"$1\" \"$2\" \"$3\" \"$4\"",
        &[&url, &pinned, dest.to_str().unwrap(), "x.tar.gz"],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(std::fs::read(&dest).unwrap(), b"pinned bytes");
}

#[test]
fn spc_building_from_another_libsodium_says_to_remove_it_and_rerun() {
    let dir = tempfile::tempdir().unwrap();
    let stale = dir.path().join("libsodium-1.0.21.tar.gz");
    std::fs::write(&stale, b"another libsodium").unwrap();
    let output = run(
        "check_spc_libsodium \"$1\" 1.0.22 \"$2\"",
        &[dir.path().to_str().unwrap(), LIBSODIUM_SHA256],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let err = stderr(&output);
    assert!(
        err.contains(&format!(
            "spc used {}, not the pinned libsodium 1.0.22",
            stale.display()
        )),
        "{err}"
    );
    assert!(
        err.contains(&format!(
            "remove {} and rerun scripts/build-php.sh",
            stale.display()
        )),
        "{err}"
    );
}

#[test]
fn spc_leaving_no_libsodium_tarball_is_reported_as_such() {
    let dir = tempfile::tempdir().unwrap();
    let output = run(
        "check_spc_libsodium \"$1\" 1.0.22 \"$2\"",
        &[dir.path().to_str().unwrap(), LIBSODIUM_SHA256],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let err = stderr(&output);
    assert!(
        err.contains(&format!(
            "spc left no libsodium tarball in {}",
            dir.path().display()
        )),
        "{err}"
    );
    assert!(!err.contains("spc used"), "{err}");
}

#[test]
fn spc_building_from_the_pinned_libsodium_passes() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("libsodium-1.0.22.tar.gz");
    std::fs::write(&tarball, b"pinned bytes").unwrap();
    let output = run(
        "check_spc_libsodium \"$1\" 1.0.22 \"$2\"",
        &[dir.path().to_str().unwrap(), &sha256(&tarball)],
    );
    assert!(output.status.success(), "{output:?}");
}
