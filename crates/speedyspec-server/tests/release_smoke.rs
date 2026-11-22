//! `scripts/release-smoke.sh`: the check a release binary passes before it is published.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn smoke(binary: &Path, component: &str, version: &str) -> Output {
    Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/release-smoke.sh"))
        .args([binary.as_os_str(), component.as_ref(), version.as_ref()])
        .output()
        .unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The FFmpeg version `scripts/build-ffmpeg.sh` pins, which a runtime's manifest must report.
fn pinned_ffmpeg() -> &'static str {
    include_str!("../../../scripts/build-ffmpeg.sh")
        .lines()
        .find_map(|line| line.strip_prefix("ffmpeg_version=\""))
        .expect("build-ffmpeg.sh pins ffmpeg_version")
        .trim_end_matches('"')
}

/// A runtime manifest with `contracts` and `sodium` as given and the pinned FFmpeg.
fn runtime_manifest(contracts: &str, sodium: &str) -> String {
    format!(
        r#"{{"version":"1.0.0","contracts":{contracts},"php":"8.5.12","sodium":"{sodium}","ffmpeg":"{}"}}"#,
        pinned_ffmpeg()
    )
}

/// A stand-in runtime whose `manifest` prints `json`.
fn fake_runtime(dir: &Path, json: &str) -> PathBuf {
    let path = dir.join("speedyspec-php-runtime");
    std::fs::write(&path, format!("#!/bin/sh\necho '{json}'\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn smoke_accepts_the_server_at_its_version() {
    let output = smoke(
        Path::new(env!("CARGO_BIN_EXE_speedyspec-server")),
        "speedyspec-server",
        env!("CARGO_PKG_VERSION"),
    );
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn smoke_refuses_a_wrong_version() {
    let output = smoke(
        Path::new(env!("CARGO_BIN_EXE_speedyspec-server")),
        "speedyspec-server",
        "9.9.9",
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("version"), "{}", stderr(&output));
}

/// The runtime's manifest has no `app`: its version, contracts, libsodium and FFmpeg are what count.
#[test]
fn smoke_runtime_needs_no_app() {
    let dir = tempfile::tempdir().unwrap();
    let good = fake_runtime(dir.path(), &runtime_manifest("[1]", "1.0.22"));
    let output = smoke(&good, "speedyspec-php-runtime", "1.0.0");
    // The stand-in is a shell script, not a static executable, so on Linux only the static check may fail.
    if cfg!(target_os = "linux") {
        assert!(stderr(&output).contains("static"), "{}", stderr(&output));
    } else {
        assert!(output.status.success(), "{}", stderr(&output));
    }

    let none = fake_runtime(dir.path(), &runtime_manifest("[]", "1.0.22"));
    let output = smoke(&none, "speedyspec-php-runtime", "1.0.0");
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("contracts"), "{}", stderr(&output));
}

/// WordPress needs libsodium: a runtime must report the version `scripts/build-php.sh` pins.
#[test]
fn smoke_runtime_needs_the_pinned_libsodium() {
    let dir = tempfile::tempdir().unwrap();
    for json in [
        r#"{"version":"1.0.0","contracts":[1],"php":"8.5.12"}"#,
        r#"{"version":"1.0.0","contracts":[1],"php":"8.5.12","sodium":"1.0.18"}"#,
    ] {
        let output = smoke(
            &fake_runtime(dir.path(), json),
            "speedyspec-php-runtime",
            "1.0.0",
        );
        assert_eq!(output.status.code(), Some(1), "{json}");
        assert!(stderr(&output).contains("libsodium"), "{}", stderr(&output));
    }
}

/// Media metadata is read with the pinned FFmpeg: a runtime must report the version `scripts/build-ffmpeg.sh` pins.
#[test]
fn smoke_runtime_needs_the_pinned_ffmpeg() {
    let dir = tempfile::tempdir().unwrap();
    for json in [
        r#"{"version":"1.0.0","contracts":[1],"php":"8.5.12","sodium":"1.0.22"}"#,
        r#"{"version":"1.0.0","contracts":[1],"php":"8.5.12","sodium":"1.0.22","ffmpeg":"0.1"}"#,
    ] {
        let output = smoke(
            &fake_runtime(dir.path(), json),
            "speedyspec-php-runtime",
            "1.0.0",
        );
        assert_eq!(output.status.code(), Some(1), "{json}");
        assert!(stderr(&output).contains("FFmpeg"), "{}", stderr(&output));
    }
}

#[test]
fn smoke_usage_exits_2() {
    let output =
        Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/release-smoke.sh"))
            .arg("only-one")
            .output()
            .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
