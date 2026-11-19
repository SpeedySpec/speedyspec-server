//! Turning a configuration into something that serves: the server's own manifest, every app's release, the
//! compatibility check, and the sockets.

use std::collections::{BTreeMap, HashMap};
use std::net::{TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use speedyspec_server_app::{Incompatibility, check_set};
use speedyspec_server_domain::{
    AccessCheck, AppId, AppKind, AppRuntime, MANIFEST_FILE, ReleaseManifest,
};
use speedyspec_server_infra_access_dev::{DenySignedIn, DevAllowAll};
use speedyspec_server_presentation::{AppHandle, LiveApps, ServeState, StaticApp};
use speedyspec_server_static::StaticSet;

use crate::config::{AccessImpl, Config, Mode};
use crate::listeners::{BoundTcp, BoundUdp};

/// What `src/release.json` holds: the server's requirements on other apps.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerRelease {
    #[serde(default)]
    requires: BTreeMap<AppId, semver::VersionReq>,
}

/// Why the server could not get ready to serve.
#[derive(Debug, thiserror::Error)]
pub enum BootError {
    #[error("reading {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("{path}: {message}")]
    Manifest { path: PathBuf, message: String },
    #[error("{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))]
    Incompatible(Vec<Incompatibility>),
    #[error("app {app}: {message}")]
    Static { app: AppId, message: String },
    #[error("app {app}: starting its PHP workers: {message}")]
    Php { app: AppId, message: String },
    #[error("binding {address}: {error}")]
    Bind {
        address: String,
        error: std::io::Error,
    },
}

/// The server's own release manifest: kind `server`, this binary's version, and the requirements in `release.json`.
pub fn server_manifest(app: AppId) -> ReleaseManifest {
    let release: ServerRelease = serde_json::from_str(include_str!("release.json"))
        .expect("src/release.json is checked by the tests");

    ReleaseManifest {
        app,
        version: crate::binary::own_version(),
        kind: AppKind::Server,
        requires: release.requires,
        static_: None,
        php: None,
        protocol: Some(crate::binary::own_protocol()),
        contracts: Vec::new(),
    }
}

/// What booting gives a generation: the serving state, and the PHP runtime its php apps run on (shared with the
/// release manager in managed mode, and with the server's own calls to the apps).
pub struct Booted {
    pub state: ServeState,
    pub php: Arc<crate::php::PhpRuntime>,
    pub binaries: Arc<crate::php::runtime_kind::RuntimeBinaries>,
}

/// Builds the serving state. Every app starts as not installed. Immutable mode then reads every baked-in release,
/// checks that they work together, loads static ones into memory and starts php ones' pools; managed mode leaves that
/// to the release manager's start-up recovery.
pub fn prepare(config: &Config) -> Result<Booted, BootError> {
    let apps = config
        .apps
        .iter()
        .map(|(id, app)| {
            let handle = if app.kind == AppKind::Server {
                AppHandle::AdminApiPlaceholder
            } else {
                AppHandle::NotInstalled
            };
            (id.clone(), handle)
        })
        .collect();
    let live = Arc::new(LiveApps::new(apps));
    let binaries = Arc::new(crate::php::runtime_kind::RuntimeBinaries::default());
    let php = Arc::new(crate::php::PhpRuntime::new(
        live.clone(),
        binaries.clone(),
        config.run_dir.clone(),
        crate::php::php_configs(config),
        config.php_runtimes().into_iter().collect(),
    ));
    if config.mode == Mode::Managed {
        return Ok(Booted {
            state: serve_state(config, live),
            php,
            binaries,
        });
    }
    let mut manifests = BTreeMap::new();
    let mut paths = HashMap::new();
    for (id, app) in &config.apps {
        let manifest = match (app.kind, &app.path) {
            (AppKind::Server, _) => server_manifest(id.clone()),
            (_, Some(path)) => {
                let manifest = baked_in_manifest(path, id, app.kind)?;
                paths.insert(id.clone(), path.clone());
                manifest
            }
            (_, None) => unreachable!("config validation requires path in immutable mode"),
        };
        manifests.insert(id.clone(), manifest);
    }
    check_set(&manifests, &config.php_runtimes()).map_err(BootError::Incompatible)?;

    // Runtimes first: the php apps' pools run their binaries.
    for (id, manifest) in manifests.iter().filter(|(_, m)| m.kind == AppKind::Runtime) {
        let binary = paths[id].join(AppKind::Runtime.binary_name().expect("a binary kind"));
        binaries.set(
            id,
            manifest.version.clone(),
            binary,
            manifest.contracts.clone(),
        );
    }
    for (id, manifest) in &manifests {
        match manifest.kind {
            AppKind::Server | AppKind::Runtime => {}
            AppKind::Php => {
                let prepared =
                    php.prepare(id, manifest, &paths[id])
                        .map_err(|message| BootError::Php {
                            app: id.clone(),
                            message,
                        })?;
                php.activate(prepared);
            }
            AppKind::Static => {
                let section = manifest
                    .static_
                    .as_ref()
                    .expect("the manifest check requires a static section");
                let set =
                    StaticSet::build(&paths[id], section, config.static_memory).map_err(|err| {
                        BootError::Static {
                            app: id.clone(),
                            message: err.to_string(),
                        }
                    })?;
                tracing::info!(event = "static_loaded", app = %id, version = %manifest.version, bytes = set.total_bytes());
                live.swap(
                    id,
                    AppHandle::Static(StaticApp {
                        current: Arc::new(set),
                        fallback: None,
                    }),
                );
            }
        }
    }
    Ok(Booted {
        state: serve_state(config, live),
        php,
        binaries,
    })
}

fn serve_state(config: &Config, live: Arc<LiveApps>) -> ServeState {
    let access: Arc<dyn AccessCheck> = match config.access {
        AccessImpl::Deny => Arc::new(DenySignedIn),
        AccessImpl::DevAllowAll => Arc::new(DevAllowAll),
    };

    ServeState {
        mounts: config.mounts.clone(),
        apps: live,
        access,
        trusted_proxies: config.trusted_proxies.clone(),
        forwarded_headers: config.forwarded_headers,
        maintenance: Arc::new(speedyspec_server_gate::MaintenanceSet::default()),
        maintenance_pages: config
            .apps
            .iter()
            .filter_map(|(id, app)| app.maintenance_page.clone().map(|page| (id.clone(), page)))
            .collect(),
    }
}

/// Every baked-in release's manifest in immutable mode, the server's included.
pub fn immutable_manifests(config: &Config) -> Result<BTreeMap<AppId, ReleaseManifest>, BootError> {
    let mut manifests = BTreeMap::new();
    for (id, app) in &config.apps {
        let manifest = match &app.path {
            _ if app.kind == AppKind::Server => server_manifest(id.clone()),
            Some(path) => baked_in_manifest(path, id, app.kind)?,
            None => continue,
        };
        manifests.insert(id.clone(), manifest);
    }
    Ok(manifests)
}

/// A baked-in release's manifest: a runtime's is printed by its binary, every other kind's is its manifest file.
fn baked_in_manifest(dir: &Path, id: &AppId, kind: AppKind) -> Result<ReleaseManifest, BootError> {
    if kind != AppKind::Runtime {
        return read_manifest(dir, id, kind);
    }
    let binary = dir.join(AppKind::Runtime.binary_name().expect("a binary kind"));
    let mut manifest = speedyspec_server_domain::BinaryInspector::manifest(
        &crate::binary::RuntimeInspector,
        &binary,
    )
    .map_err(|message| BootError::Manifest {
        path: binary.clone(),
        message,
    })?;
    manifest.app = id.clone();
    Ok(manifest)
}

fn read_manifest(dir: &Path, id: &AppId, kind: AppKind) -> Result<ReleaseManifest, BootError> {
    let path = dir.join(MANIFEST_FILE);
    let bytes = std::fs::read(&path).map_err(|err| BootError::Io(path.clone(), err))?;
    let manifest = ReleaseManifest::parse(&bytes).map_err(|err| BootError::Manifest {
        path: path.clone(),
        message: err.to_string(),
    })?;
    if manifest.app != *id {
        return Err(BootError::Manifest {
            path,
            message: format!(
                "app is {:?}, but the config names {:?}",
                manifest.app.as_str(),
                id.as_str()
            ),
        });
    }
    if manifest.kind != kind {
        return Err(BootError::Manifest {
            path,
            message: format!(
                "kind is {:?}, but the config says {:?}",
                manifest.kind.as_str(),
                kind.as_str()
            ),
        });
    }

    Ok(manifest)
}

/// Binds every configured listener, and its UDP socket on the same port when `http3` is set.
pub fn bind(config: &Config) -> Result<(Vec<BoundTcp>, Vec<BoundUdp>), BootError> {
    let mut tcp = Vec::new();
    let mut udp = Vec::new();
    for listener in &config.listeners {
        let bound = TcpListener::bind(listener.address).map_err(|error| BootError::Bind {
            address: listener.address.to_string(),
            error,
        })?;
        let local = bound.local_addr().map_err(|error| BootError::Bind {
            address: listener.address.to_string(),
            error,
        })?;
        tracing::info!(event = "listening", address = %local, protocol = "tcp");
        if listener.http3 {
            let tls = listener
                .tls
                .clone()
                .expect("config validation requires tls for http3");
            let socket = UdpSocket::bind(local).map_err(|error| BootError::Bind {
                address: local.to_string(),
                error,
            })?;
            tracing::info!(event = "listening", address = %local, protocol = "udp");
            udp.push(BoundUdp { socket, tls });
        }
        tcp.push(BoundTcp {
            listener: bound,
            tls: listener.tls.clone(),
        });
    }

    Ok((tcp, udp))
}
