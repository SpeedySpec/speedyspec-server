//! A binary inspector for tests: the "binary" is a file holding its manifest as JSON.

use std::path::Path;

use speedyspec_server_domain::{BinaryInspector, ReleaseManifest};

pub struct FakeInspector;

impl BinaryInspector for FakeInspector {
    fn manifest(&self, binary: &Path) -> Result<ReleaseManifest, String> {
        let bytes = std::fs::read(binary).map_err(|err| format!("{}: {err}", binary.display()))?;
        ReleaseManifest::parse(&bytes).map_err(|err| err.to_string())
    }
}
