#![allow(dead_code)]
//! A release manager over a temp-dir store, a memory source, state store and verifier, and a recording runtime.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use semver::Version;
use sha2::{Digest, Sha256};
use speedyspec_server_app::{AppPolicy, ReleaseManager, Signing};
use speedyspec_server_domain::{AppId, AppKind, AppRuntime, ReleaseManifest, ReleaseState};
use speedyspec_server_infra_fs::FsReleaseStore;
use speedyspec_server_infra_memory::archive::release;
use speedyspec_server_infra_memory::{
    MemorySource, MemoryStateStore, MemoryVerifier, RecordingRuntime,
};

pub const SERVER_VERSION: &str = "3.2.1";

pub fn id(app: &str) -> AppId {
    AppId::parse(app).unwrap()
}

pub fn v(version: &str) -> Version {
    Version::parse(version).unwrap()
}

pub struct World {
    pub dir: tempfile::TempDir,
    pub source: Arc<MemorySource>,
    pub state: Arc<MemoryStateStore>,
    pub runtime: Arc<RecordingRuntime>,
    pub policies: BTreeMap<AppId, AppPolicy>,
    /// (app, version) -> (manifest json, files)
    releases: BTreeMap<(String, String), String>,
}

pub fn url(app: &str) -> String {
    format!("https://releases.test/{app}/index.json")
}

fn artifact_url(app: &str, version: &str) -> String {
    format!("https://releases.test/{app}/{app}-{version}.tar.zst")
}

impl World {
    pub fn new() -> World {
        World::with_state(ReleaseState::default())
    }

    pub fn with_state(state: ReleaseState) -> World {
        World {
            dir: tempfile::tempdir().unwrap(),
            source: Arc::new(MemorySource::new()),
            state: Arc::new(MemoryStateStore::with(state)),
            runtime: Arc::new(RecordingRuntime::new()),
            policies: BTreeMap::new(),
            releases: BTreeMap::new(),
        }
    }

    pub fn app(&mut self, app: &str, kind: AppKind, keep: u32) -> &mut AppPolicy {
        self.policies.insert(
            id(app),
            AppPolicy {
                kind,
                source: Some(url(app)),
                keep,
                prerelease: false,
                signing: Signing::Verify,
            },
        );
        self.policies.get_mut(&id(app)).unwrap()
    }

    /// Publishes a php release of `app` requiring `requires`, and rewrites the app's index.
    pub fn publish(&mut self, app: &str, version: &str, requires: &str) {
        let manifest = format!(
            r#"{{"app":"{app}","version":"{version}","kind":"php","requires":{{{requires}}}}}"#
        );
        self.publish_manifest(app, version, &manifest);
    }

    pub fn publish_manifest(&mut self, app: &str, version: &str, manifest: &str) {
        let bytes = release(manifest, &[("index.php", version)]);
        let sha = hex(&Sha256::digest(&bytes));
        self.source.add_file(&artifact_url(app, version), bytes);
        self.source.add_file(
            &format!("{}.minisig", artifact_url(app, version)),
            b"good".to_vec(),
        );
        self.releases
            .insert((app.to_string(), version.to_string()), sha);
        self.write_index(app);
    }

    /// Replaces a published release's checksum in the index.
    pub fn corrupt_checksum(&mut self, app: &str, version: &str) {
        self.releases
            .insert((app.to_string(), version.to_string()), "0".repeat(64));
        self.write_index(app);
    }

    pub fn bad_signature(&self, app: &str, version: &str) {
        self.source.add_file(
            &format!("{}.minisig", artifact_url(app, version)),
            b"bad".to_vec(),
        );
    }

    pub fn write_index(&self, app: &str) {
        let entries: Vec<String> = self
            .releases
            .iter()
            .filter(|((a, _), _)| a == app)
            .map(|((_, version), sha)| {
                format!(
                    r#"{{"version":"{version}","published":"x","artifacts":[{{"target":"any","url":"{}","sha256":"{sha}","minisig":"{}.minisig"}}]}}"#,
                    artifact_url(app, version),
                    artifact_url(app, version)
                )
            })
            .collect();
        self.source.add_index(
            &url(app),
            &format!(r#"{{"app":"{app}","releases":[{}]}}"#, entries.join(",")),
        );
    }

    pub fn artifact_url(&self, app: &str, version: &str) -> String {
        artifact_url(app, version)
    }

    pub fn manager(&self) -> ReleaseManager {
        let runtime: Arc<dyn AppRuntime> = self.runtime.clone();
        let runtimes: HashMap<AppKind, Arc<dyn AppRuntime>> =
            [(AppKind::Php, runtime.clone()), (AppKind::Static, runtime)]
                .into_iter()
                .collect();
        ReleaseManager::new(
            self.policies.clone(),
            server_manifest(),
            self.source.clone(),
            Arc::new(MemoryVerifier),
            Arc::new(FsReleaseStore::new(self.dir.path())),
            self.state.clone(),
            runtimes,
        )
        .unwrap()
    }

    /// The state as last saved.
    pub fn saved(&self) -> ReleaseState {
        speedyspec_server_domain::StateStore::load(&*self.state).unwrap()
    }

    pub fn release_dir(&self, app: &str, version: &str) -> std::path::PathBuf {
        self.dir.path().join("releases").join(app).join(version)
    }
}

pub fn server_manifest() -> ReleaseManifest {
    ReleaseManifest {
        app: id("server"),
        version: v(SERVER_VERSION),
        kind: AppKind::Server,
        requires: Default::default(),
        static_: None,
        php: None,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
