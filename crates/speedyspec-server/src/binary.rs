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

/// This binary's version. With the `test-hooks` feature, a file `<exe>.test-version` next to the executable
/// overrides it, so tests can run several "versions" of one build.
pub fn own_version() -> semver::Version {
    #[cfg(feature = "test-hooks")]
    if let Some(text) = hook("test-version")
        && let Ok(version) = semver::Version::parse(text.trim())
    {
        return version;
    }
    semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is SemVer")
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

/// Under `test-hooks`: how long after `Ready` a generation exits on its own (`<exe>.test-exit-after-ready-ms`).
pub fn exit_after_ready() -> Option<Duration> {
    #[cfg(feature = "test-hooks")]
    if let Some(text) = hook("test-exit-after-ready-ms") {
        return text.trim().parse().ok().map(Duration::from_millis);
    }
    None
}

/// Under `test-hooks`: a generation that never reports ready (`<exe>.test-never-ready`).
pub fn never_ready() -> bool {
    #[cfg(feature = "test-hooks")]
    if hook("test-never-ready").is_some() {
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
