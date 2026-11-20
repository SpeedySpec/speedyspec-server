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
    /// With the gate: what its revocation poll needs.
    pub revocation: Option<Revocation>,
}

/// What the gate's revocation poll needs (started by the generation, on its runtime).
pub struct Revocation {
    pub gate: Arc<speedyspec_server_gate::GateAccess>,
    pub app: AppId,
    pub every: std::time::Duration,
    pub calls: Arc<dyn speedyspec_server_gate::GateCalls>,
    pub sessions: Arc<speedyspec_server_gate::SessionCache>,
}

impl Revocation {
    /// Polls the gate app's live release's revocation feed.
    pub fn spawn(self, php: Arc<crate::php::PhpRuntime>) {
        let app = self.app.clone();
        let path: speedyspec_server_gate::ChangesPath =
            Arc::new(move || Some(php.live_manifest(&app)?.php_section()?.gate?.changes));
        speedyspec_server_gate::RevocationPoll::spawn(
            self.app,
            path,
            self.every,
            self.calls,
            self.sessions,
        );
        // The site state's maintenance applies whether or not anyone visits a gated mount.
        let (gate, every) = (self.gate, self.every);
        tokio::spawn(async move {
            let mut ticks = tokio::time::interval(every);
            loop {
                ticks.tick().await;
                gate.refresh_site().await;
            }
        });
    }
}

/// Builds the serving state. Every app starts as not installed. Immutable mode then reads every baked-in release,
/// checks that they work together, loads static ones into memory and starts php ones' pools; managed mode leaves that
/// to the release manager's start-up recovery.
pub fn prepare(config: &Config) -> Result<Booted, BootError> {
    let apps = config
        .apps
        .iter()
        .map(|(id, _)| (id.clone(), AppHandle::NotInstalled))
        .collect();
    let live = Arc::new(LiveApps::new(apps));
    let binaries = Arc::new(crate::php::runtime_kind::RuntimeBinaries::default());
    let php = Arc::new(
        crate::php::PhpRuntime::new(
            live.clone(),
            binaries.clone(),
            config.run_dir.clone(),
            crate::php::php_configs(config),
            config.php_runtimes().into_iter().collect(),
        )
        .with_gate_app(config.gate.as_ref().map(|gate| gate.app.clone())),
    );
    if config.mode == Mode::Managed {
        let (state, revocation) = serve_state(config, live, &php);
        return Ok(Booted {
            state,
            php,
            binaries,
            revocation,
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
                let rules = speedyspec_server_gate::AccessRules::compile(&section.access).map_err(
                    |message| BootError::Static {
                        app: id.clone(),
                        message,
                    },
                )?;
                live.set_rules(id, rules);
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
    let (state, revocation) = serve_state(config, live, &php);
    Ok(Booted {
        state,
        php,
        binaries,
        revocation,
    })
}

/// What the gate reads of the live releases: the gate app's endpoints from its live php release, and every app's
/// access rules from the live table.
struct LiveGateView {
    php: Arc<crate::php::PhpRuntime>,
    live: Arc<LiveApps>,
    gate_app: AppId,
}

impl speedyspec_server_gate::GateView for LiveGateView {
    fn gate(&self) -> Option<speedyspec_server_domain::GateSection> {
        self.php.live_manifest(&self.gate_app)?.php_section()?.gate
    }

    fn rules(&self, app: &AppId) -> Option<Arc<speedyspec_server_gate::AccessRules>> {
        self.live.rules(app)
    }
}

fn serve_state(
    config: &Config,
    live: Arc<LiveApps>,
    php: &Arc<crate::php::PhpRuntime>,
) -> (ServeState, Option<Revocation>) {
    let maintenance = Arc::new(speedyspec_server_gate::MaintenanceSet::default());
    let mut revocation = None;
    let access: Arc<dyn AccessCheck> = match (config.access, &config.gate) {
        (AccessImpl::Deny, _) => Arc::new(DenySignedIn),
        (AccessImpl::DevAllowAll, _) => Arc::new(DevAllowAll),
        (AccessImpl::Gate, gate) => {
            let settings = gate.clone().expect("the config check requires [gate]");
            let view = Arc::new(LiveGateView {
                php: php.clone(),
                live: live.clone(),
                gate_app: settings.app.clone(),
            });
            let calls: Arc<dyn speedyspec_server_gate::GateCalls> =
                Arc::new(crate::php::calls::FastCgiGateCalls::new(
                    php.clone(),
                    &config.mounts,
                    settings.timeout,
                ));
            let gate = Arc::new(
                speedyspec_server_gate::GateAccess::new(
                    settings.clone(),
                    view,
                    calls.clone(),
                    maintenance.clone(),
                )
                .with_apps(config.apps.keys().cloned().collect())
                .with_admin(
                    config
                        .mounts
                        .iter()
                        .filter(|mount| mount.access == speedyspec_server_domain::Access::SignedIn)
                        .map(|mount| mount.app.clone())
                        .collect(),
                ),
            );
            let sessions = gate.sessions().clone();
            {
                let sessions = sessions.clone();
                php.set_sign_out(Arc::new(move |headers, set_cookies| {
                    if let Some(cookie) = speedyspec_server_gate::cookie_header(headers)
                        && speedyspec_server_gate::clears_a_sent_cookie(&cookie, set_cookies)
                    {
                        sessions.forget_header(&cookie);
                    }
                }));
            }
            revocation = Some(Revocation {
                gate: gate.clone(),
                app: settings.app.clone(),
                every: settings.poll,
                calls,
                sessions,
            });
            gate
        }
    };

    let state = ServeState {
        mounts: config.mounts.clone(),
        apps: live,
        access,
        trusted_proxies: config.trusted_proxies.clone(),
        forwarded_headers: config.forwarded_headers,
        maintenance,
        maintenance_pages: config
            .apps
            .iter()
            .filter_map(|(id, app)| app.maintenance_page.clone().map(|page| (id.clone(), page)))
            .collect(),
        gate_app: config.gate.as_ref().map(|gate| gate.app.clone()),
    };
    (state, revocation)
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
