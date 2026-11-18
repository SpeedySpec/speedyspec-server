//! Turning a configuration into something that serves: the server's own manifest, every app's release, the
//! compatibility check, and the sockets.

use std::collections::{BTreeMap, HashMap};
use std::net::{TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use speedyspec_server_app::{Incompatibility, check_set};
use speedyspec_server_domain::{AccessCheck, AppId, AppKind, MANIFEST_FILE, ReleaseManifest};
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
        version: semver::Version::parse(env!("CARGO_PKG_VERSION"))
            .expect("the crate version is SemVer"),
        kind: AppKind::Server,
        requires: release.requires,
        static_: None,
        php: None,
    }
}

/// Builds the serving state. Immutable mode reads every baked-in release, checks that they work together, and loads
/// static ones into memory. Managed mode starts every app as not installed; the release manager's start-up recovery
/// then activates what is kept.
pub fn prepare(config: &Config) -> Result<ServeState, BootError> {
    if config.mode == Mode::Managed {
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
        return Ok(serve_state(config, apps));
    }
    let mut manifests = BTreeMap::new();
    let mut paths = HashMap::new();
    for (id, app) in &config.apps {
        let manifest = match (app.kind, &app.path) {
            (AppKind::Server, _) => server_manifest(id.clone()),
            (_, Some(path)) => {
                let manifest = read_manifest(path, id, app.kind)?;
                paths.insert(id.clone(), path.clone());
                manifest
            }
            (_, None) => unreachable!("config validation requires path in immutable mode"),
        };
        manifests.insert(id.clone(), manifest);
    }
    check_set(&manifests).map_err(BootError::Incompatible)?;

    let mut apps = HashMap::new();
    for (id, manifest) in &manifests {
        let handle = match manifest.kind {
            AppKind::Server => AppHandle::AdminApiPlaceholder,
            AppKind::Php => AppHandle::PhpPlaceholder,
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
                AppHandle::Static(StaticApp {
                    current: Arc::new(set),
                    fallback: None,
                })
            }
        };
        apps.insert(id.clone(), handle);
    }
    Ok(serve_state(config, apps))
}

fn serve_state(config: &Config, apps: HashMap<AppId, AppHandle>) -> ServeState {
    let access: Arc<dyn AccessCheck> = match config.access {
        AccessImpl::Deny => Arc::new(DenySignedIn),
        AccessImpl::DevAllowAll => Arc::new(DevAllowAll),
    };

    ServeState {
        mounts: config.mounts.clone(),
        apps: Arc::new(LiveApps::new(apps)),
        access,
        trusted_proxies: config.trusted_proxies.clone(),
        forwarded_headers: config.forwarded_headers,
    }
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
