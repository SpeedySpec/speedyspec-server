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
