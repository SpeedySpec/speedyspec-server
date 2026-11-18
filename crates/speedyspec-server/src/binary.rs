//! Server binaries: reading a binary's manifest by running it.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use speedyspec_server_domain::{BinaryInspector, ReleaseManifest};

/// The Rust target triple this binary was built for.
pub const TARGET: &str = env!("SPEEDYSPEC_TARGET");

/// How long `<binary> manifest` may take.
const INSPECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Reads a server binary's manifest by running `<binary> manifest`.
pub struct ProcessInspector;

impl BinaryInspector for ProcessInspector {
    fn manifest(&self, binary: &Path) -> Result<ReleaseManifest, String> {
        let mut child = Command::new(binary)
            .arg("manifest")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| format!("running {} manifest: {err}", binary.display()))?;
        let deadline = Instant::now() + INSPECT_TIMEOUT;
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
                break status;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{} manifest did not finish within {INSPECT_TIMEOUT:?}",
                    binary.display()
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let mut out = Vec::new();
        child
            .stdout
            .take()
            .expect("piped")
            .read_to_end(&mut out)
            .map_err(|err| err.to_string())?;
        if !status.success() {
            return Err(format!(
                "{} manifest exited with {status}",
                binary.display()
            ));
        }
        ReleaseManifest::parse(&out).map_err(|err| format!("{} manifest: {err}", binary.display()))
    }
}

/// This binary's version. With the `test-hooks` feature: a file `<exe>.test-version` next to the executable
/// overrides it; or, with `SPEEDYSPEC_TEST_VERSION_FROM_DIR` set, the name of the executable's directory does when it
/// is a version (kept server binaries live in `releases/server/<version>/`).
pub fn own_version() -> semver::Version {
    #[cfg(feature = "test-hooks")]
    if let Some(version) = test_version() {
        return version;
    }
    semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is SemVer")
}

#[cfg(feature = "test-hooks")]
fn test_version() -> Option<semver::Version> {
    if let Some(text) = hook("test-version") {
        return semver::Version::parse(text.trim()).ok();
    }
    if std::env::var_os("SPEEDYSPEC_TEST_VERSION_FROM_DIR").is_some() {
        let exe = std::env::current_exe().ok()?;
        let dir = exe.parent()?.file_name()?.to_str()?;
        return semver::Version::parse(dir).ok();
    }
    None
}

/// Under `test-hooks`: whether `SPEEDYSPEC_TEST_<NAME>_VERSION` names this binary's version.
#[cfg(feature = "test-hooks")]
fn env_hook_applies(name: &str) -> bool {
    std::env::var(format!("SPEEDYSPEC_TEST_{name}_VERSION"))
        .is_ok_and(|v| v.trim() == own_version().to_string())
}

/// The keeper protocol this binary speaks (`<exe>.test-protocol` overrides it under `test-hooks`).
pub fn own_protocol() -> u32 {
    #[cfg(feature = "test-hooks")]
    if let Some(text) = hook("test-protocol")
        && let Ok(protocol) = text.trim().parse()
    {
        return protocol;
    }
    speedyspec_server_handoff::PROTOCOL
}

/// Under `test-hooks`: how long after `Ready` a generation exits on its own (`<exe>.test-exit-after-ready-ms`, or
/// `SPEEDYSPEC_TEST_EXIT_AFTER_READY_MS` when `SPEEDYSPEC_TEST_EXIT_VERSION` is this version).
pub fn exit_after_ready() -> Option<Duration> {
    #[cfg(feature = "test-hooks")]
    {
        if let Some(text) = hook("test-exit-after-ready-ms") {
            return text.trim().parse().ok().map(Duration::from_millis);
        }
        if env_hook_applies("EXIT") {
            let ms = std::env::var("SPEEDYSPEC_TEST_EXIT_AFTER_READY_MS")
                .ok()?
                .trim()
                .parse()
                .ok()?;
            return Some(Duration::from_millis(ms));
        }
    }
    None
}

/// Under `test-hooks`: a generation that never reports ready (`<exe>.test-never-ready`, or
/// `SPEEDYSPEC_TEST_NEVER_READY_VERSION`).
pub fn never_ready() -> bool {
    #[cfg(feature = "test-hooks")]
    if hook("test-never-ready").is_some() || env_hook_applies("NEVER_READY") {
        return true;
    }
    false
}

/// Under `test-hooks`: a generation that exits before reporting ready (`SPEEDYSPEC_TEST_CRASH_VERSION`).
pub fn crash_before_ready() -> bool {
    #[cfg(feature = "test-hooks")]
    if env_hook_applies("CRASH") {
        return true;
    }
    false
}

#[cfg(feature = "test-hooks")]
fn hook(name: &str) -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let mut path = exe.into_os_string();
    path.push(format!(".{name}"));
    std::fs::read_to_string(std::path::PathBuf::from(path)).ok()
}
