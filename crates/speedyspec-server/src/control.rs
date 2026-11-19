//! The control socket: one JSON request line and one JSON response line per connection, at
//! `<run_dir>/control.sock` (owner-only). The `release` and `status` commands are its client.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use speedyspec_server_app::ReleaseManager;
use speedyspec_server_domain::{AppId, AppKind, ReleaseManifest};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

/// The message every change is refused with in immutable mode.
pub const IMMUTABLE_REFUSAL: &str =
    "mode = \"immutable\": releases are baked into the image; upgrade with the speedyspec CLI";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlRequest {
    List {
        app: Option<AppId>,
    },
    Check {
        app: Option<AppId>,
    },
    Install {
        app: AppId,
        version: Option<Version>,
    },
    Switch {
        targets: Vec<(AppId, Version)>,
    },
    Rollback {
        app: AppId,
        version: Option<Version>,
    },
    Pin {
        app: AppId,
        version: Version,
    },
    Unpin {
        app: AppId,
    },
    Status,
    /// Switch an app again to the release whose migration failed.
    RetryMigration {
        app: AppId,
    },
    /// Turn an operator's maintenance on or off, or clear every recorded maintenance (a failed migration's too).
    Maintenance {
        action: MaintenanceAction,
        apps: Vec<AppId>,
    },
    /// Hand off to a new generation of the confirmed binary, re-reading the config (answered by the keeper).
    Reload,
    /// Internal: the keeper refused a server switch the generation recorded; undo it without marking it failed.
    CancelServerSwitch {
        version: Version,
    },
    /// Internal: the keeper confirms a server trial through the generation that passed it.
    ConfirmServer {
        version: Version,
    },
}

/// `Ok` carries either a line to print (mutating commands) or the data `list`, `check` and `status` format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ControlResponse {
    Ok(Value),
    Error(String),
}

/// Answers control requests. Called on a blocking thread.
/// What `speedyspec-server maintenance` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MaintenanceAction {
    On,
    Off,
    Clear,
}

pub trait ControlBackend: Send + Sync {
    fn handle(&self, request: ControlRequest) -> ControlResponse;
}

/// One app's row in `list` and `status`.
fn row(
    app: &AppId,
    kind: AppKind,
    active: Option<&Version>,
    kept: &[Version],
    pinned: Option<&Version>,
    failed: &[Version],
) -> Value {
    json!({
        "app": app,
        "kind": kind.as_str(),
        "active": active.map(ToString::to_string),
        "kept": kept.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "pinned": pinned.map(ToString::to_string),
        "failed": failed.iter().map(ToString::to_string).collect::<Vec<_>>(),
    })
}

fn listing(server: &Version, mode: &str, apps: Vec<Value>, incompatible: Vec<String>) -> Value {
    json!({ "server": { "version": server.to_string(), "mode": mode }, "apps": apps, "incompatible": incompatible })
}

/// Managed mode: every request runs through the release manager.
pub struct ManagedBackend {
    pub manager: Arc<ReleaseManager>,
    /// Set while this generation drains: release commands are refused.
    pub draining: Arc<std::sync::atomic::AtomicBool>,
    /// Every configured app and its kind.
    pub apps: BTreeMap<AppId, AppKind>,
    pub server_version: Version,
}

impl ManagedBackend {
    fn rows(&self, only: Option<&AppId>) -> Result<Vec<Value>, String> {
        if let Some(app) = only
            && !self.apps.contains_key(app)
        {
            return Err(format!("{app} is not a configured app"));
        }
        let state = self.manager.status();
        Ok(self
            .apps
            .iter()
            .filter(|(app, kind)| **kind != AppKind::Server && only.is_none_or(|only| only == *app))
            .map(|(app, kind)| {
                let current = state.apps.get(app).cloned().unwrap_or_default();
                let failed: Vec<Version> = current.failed.keys().cloned().collect();
                row(
                    app,
                    *kind,
                    current.active.as_ref(),
                    &current.kept,
                    current.pinned.as_ref(),
                    &failed,
                )
            })
            .collect())
    }
}

impl ControlBackend for ManagedBackend {
    fn handle(&self, request: ControlRequest) -> ControlResponse {
        let manager = &self.manager;
        let reads = matches!(
            request,
            ControlRequest::List { .. } | ControlRequest::Status | ControlRequest::Check { .. }
        );
        if !reads
            && !matches!(
                request,
                ControlRequest::ConfirmServer { .. } | ControlRequest::CancelServerSwitch { .. }
            )
            && self.draining.load(std::sync::atomic::Ordering::SeqCst)
        {
            return ControlResponse::Error(
                "a server upgrade is in progress; try again once it has been confirmed or has failed"
                    .to_string(),
            );
        }
        let server = manager.server_app().clone();
        let upgrade = |targets: &[(AppId, Version)]| -> Result<Value, String> {
            let upgrade = manager
                .begin_server_switch(targets)
                .map_err(|err| err.to_string())?;
            if let Some(order) = &upgrade.rollout {
                // In a cluster the pods upgrade themselves, turn by turn; nothing happens here now.
                return Ok(json!(format!(
                    "rolling out server {} across {} pods ({}), one at a time",
                    upgrade.version,
                    order.len(),
                    order.join(", ")
                )));
            }
            // Until the keeper resumes or cancels this generation, its state must not change under the new one,
            // which has already read `state.json`.
            self.draining
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(
                json!({ "upgrade": { "version": upgrade.version.to_string(), "binary": upgrade.binary } }),
            )
        };
        let result: Result<Value, String> = match request {
            ControlRequest::Switch { targets } if targets.iter().any(|(app, _)| *app == server) => {
                upgrade(&targets)
            }
            ControlRequest::Rollback { app, version } if app == server => {
                match version.or_else(|| manager.previous_server()) {
                    Some(version) => upgrade(&[(app, version)]),
                    None => Err(format!(
                        "{app}: no older server binary is kept to roll back to"
                    )),
                }
            }
            ControlRequest::Reload => Err("reload is answered by the keeper".to_string()),
            ControlRequest::CancelServerSwitch { version } => {
                let cancelled = manager
                    .cancel_pending_group(&version)
                    .map(|()| json!(format!("cancelled server {version}")))
                    .map_err(|err| err.to_string());
                self.draining
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                cancelled
            }
            ControlRequest::ConfirmServer { .. } if crate::binary::confirm_fails() => {
                Err("test hook: confirming the trial fails".to_string())
            }
            ControlRequest::ConfirmServer { version } => manager
                .confirm_server_trial(&version)
                .map(|()| json!(format!("confirmed server {version}")))
                .map_err(|err| err.to_string()),
            ControlRequest::List { app } => self
                .rows(app.as_ref())
                .map(|rows| listing(&self.server_version, "managed", rows, Vec::new())),
            ControlRequest::Status => self.rows(None).map(|rows| {
                let incompatible = manager
                    .last_boot()
                    .map(|report| {
                        report
                            .incompatible
                            .iter()
                            .map(ToString::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                let mut value = listing(&self.server_version, "managed", rows, incompatible);
                if let Some(cluster) = manager.cluster_status() {
                    value["cluster"] = json!({
                        "pod": cluster.pod,
                        "leader": cluster.leader,
                        "leader_name": cluster.leader_name,
                        "members": cluster.members.iter().map(|m| json!({
                            "pod": m.pod,
                            "live": m.live,
                            "serving": m.serving,
                            "ready": m.ready,
                            "server": m.server.as_ref().map(ToString::to_string),
                        })).collect::<Vec<_>>(),
                    });
                }
                value
            }),
            ControlRequest::Check { app } => {
                let apps: Vec<AppId> = match app {
                    Some(app) => vec![app],
                    None => self
                        .apps
                        .iter()
                        .filter(|(_, k)| **k != AppKind::Server)
                        .map(|(a, _)| a.clone())
                        .collect(),
                };
                let results: Vec<Value> = apps
                    .iter()
                    .map(|app| match manager.check(app) {
                        Ok(candidate) => {
                            json!({ "app": app, "candidate": candidate.map(|v| v.to_string()) })
                        }
                        Err(err) => json!({ "app": app, "error": err.to_string() }),
                    })
                    .collect();
                Ok(json!({ "check": results }))
            }
            ControlRequest::Install { app, version } => manager
                .install(&app, version)
                .map(|version| json!(format!("installed {app} {version}")))
                .map_err(|err| err.to_string()),
            ControlRequest::Switch { targets } => manager
                .switch(&targets)
                .map_err(|err| err.to_string())
                .map(|()| {
                    let names: Vec<String> = targets
                        .iter()
                        .map(|(app, version)| format!("{app} {version}"))
                        .collect();
                    json!(format!("switched {}", names.join(", ")))
                }),
            ControlRequest::Rollback { app, version } => manager
                .rollback(&app, version)
                .map(|version| json!(format!("rolled back {app} to {version}")))
                .map_err(|err| err.to_string()),
            ControlRequest::Pin { app, version } => manager
                .pin(&app, version.clone())
                .map(|()| json!(format!("pinned {app} {version}")))
                .map_err(|err| err.to_string()),
            ControlRequest::Unpin { app } => manager
                .unpin(&app)
                .map(|()| json!(format!("unpinned {app}")))
                .map_err(|err| err.to_string()),
            ControlRequest::RetryMigration { app } => manager
                .retry_migration(&app)
                .map(|()| json!(format!("migrated and switched {app}")))
                .map_err(|err| err.to_string()),
            ControlRequest::Maintenance { action, apps } => {
                let done = match action {
                    MaintenanceAction::On => manager.set_maintenance(&apps, true),
                    MaintenanceAction::Off => manager.set_maintenance(&apps, false),
                    MaintenanceAction::Clear => manager.clear_maintenance(&apps),
                };
                let word = match action {
                    MaintenanceAction::On => "on",
                    MaintenanceAction::Off => "off",
                    MaintenanceAction::Clear => "cleared",
                };
                done.map(|apps| {
                    let names: Vec<&str> = apps.iter().map(AppId::as_str).collect();
                    json!(format!("maintenance {word}: {}", names.join(", ")))
                })
                .map_err(|err| err.to_string())
            }
        };
        match result {
            Ok(value) => ControlResponse::Ok(value),
            Err(message) => ControlResponse::Error(message),
        }
    }
}

/// Immutable mode: reports the baked-in releases and refuses every change.
pub struct ImmutableBackend {
    pub manifests: BTreeMap<AppId, ReleaseManifest>,
    pub server_version: Version,
}

impl ControlBackend for ImmutableBackend {
    fn handle(&self, request: ControlRequest) -> ControlResponse {
        let rows = |only: Option<&AppId>| -> Vec<Value> {
            self.manifests
                .values()
                .filter(|m| m.kind != AppKind::Server && only.is_none_or(|only| *only == m.app))
                .map(|m| {
                    row(
                        &m.app,
                        m.kind,
                        Some(&m.version),
                        std::slice::from_ref(&m.version),
                        None,
                        &[],
                    )
                })
                .collect()
        };
        match request {
            ControlRequest::List { app } => ControlResponse::Ok(listing(
                &self.server_version,
                "immutable",
                rows(app.as_ref()),
                Vec::new(),
            )),
            ControlRequest::Status => ControlResponse::Ok(listing(
                &self.server_version,
                "immutable",
                rows(None),
                Vec::new(),
            )),
            _ => ControlResponse::Error(IMMUTABLE_REFUSAL.to_string()),
        }
    }
}

/// Where the keeper's control socket lives: the same path across every handoff.
pub fn socket_path(run_dir: &Path) -> PathBuf {
    run_dir.join("keeper.sock")
}

/// Answers one raw request line through `backend`, as a raw response line (no newline).
pub fn answer(backend: &dyn ControlBackend, line: &str) -> String {
    let response = match serde_json::from_str::<ControlRequest>(line) {
        Ok(request) => backend.handle(request),
        Err(err) => ControlResponse::Error(format!("not a control request: {err}")),
    };
    serde_json::to_string(&response).expect("responses serialize")
}

/// Answers raw control request lines. The keeper forwards them to its active generation.
pub trait LineHandler: Send + Sync {
    fn handle_line(
        &self,
        line: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send + '_>>;
}

/// Binds the control socket (replacing a stale one, refusing a live one), makes it owner-only, and serves it until
/// the process ends, one request line and one response line per connection. With `create_dir` false (immutable
/// mode, which never writes to disk) the socket's directory must already exist, for example a tmpfs or an `emptyDir`
/// volume.
pub fn serve(
    path: &Path,
    handler: Arc<dyn LineHandler>,
    create_dir: bool,
) -> std::io::Result<std::os::fd::OwnedFd> {
    if create_dir && let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::os::unix::net::UnixStream::connect(path).is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!(
                "another speedyspec-server is already running on {}",
                path.display()
            ),
        ));
    }
    // Bound under a temporary name and made owner-only before it appears at `path`, so the socket is never
    // reachable with looser permissions.
    let staging = path.with_extension(format!("sock.{}", std::process::id()));
    let _ = std::fs::remove_file(&staging);
    let listener = std::os::unix::net::UnixListener::bind(&staging)?;
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&staging, path)?;
    let copy = std::os::fd::OwnedFd::from(listener.try_clone()?);
    serve_listener(listener, handler)?;
    Ok(copy)
}

/// Serves an already-bound control socket (the keeper adopts its own across a re-exec).
pub fn serve_listener(
    listener: std::os::unix::net::UnixListener,
    handler: Arc<dyn LineHandler>,
) -> std::io::Result<()> {
    listener.set_nonblocking(true)?;
    let listener = tokio::net::UnixListener::from_std(listener)?;
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let handler = handler.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut line = String::new();
                if tokio::io::BufReader::new(read)
                    .read_line(&mut line)
                    .await
                    .is_err()
                {
                    return;
                }
                let mut out = handler.handle_line(line.trim_end().to_string()).await;
                out.push('\n');
                let _ = write.write_all(out.as_bytes()).await;
            });
        }
    });
    Ok(())
}

/// Sends one request to the server's control socket and waits for the answer.
pub fn request(path: &Path, request: &ControlRequest) -> Result<ControlResponse, String> {
    let read_only = matches!(
        request,
        ControlRequest::Status | ControlRequest::List { .. } | ControlRequest::Check { .. }
    );
    match request_once(path, request) {
        // The keeper closed the connection without answering: it re-executed itself (same socket) or a generation
        // was replaced. A read-only request is asked once more; anything else may already have happened.
        Ok(None) if read_only => {
            std::thread::sleep(std::time::Duration::from_millis(200));
            request_once(path, request)?.ok_or_else(|| {
                "the server closed the connection without answering; try again".to_string()
            })
        }
        Ok(None) => Err(
            "the server closed the connection without answering; check `status` before retrying"
                .to_string(),
        ),
        Ok(Some(response)) => Ok(response),
        Err(err) => Err(err),
    }
}

/// One request; `Ok(None)` when the connection closed before an answer.
fn request_once(path: &Path, request: &ControlRequest) -> Result<Option<ControlResponse>, String> {
    let unreachable = |err: std::io::Error| {
        format!(
            "cannot reach the server at {}: {err}; is it running?",
            path.display()
        )
    };
    let mut stream = std::os::unix::net::UnixStream::connect(path).map_err(unreachable)?;
    let mut line = serde_json::to_string(request).expect("requests serialize");
    line.push('\n');
    stream.write_all(line.as_bytes()).map_err(unreachable)?;
    let mut answer = String::new();
    BufReader::new(stream)
        .read_line(&mut answer)
        .map_err(unreachable)?;
    if answer.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&answer)
        .map(Some)
        .map_err(|err| format!("the server sent an unreadable answer: {err}"))
}

/// Formats an `Ok` answer for stdout: one line per app (`app kind active kept pinned failed`, tab-separated, `-` for
/// none), the server line first for `status`, or the message of a mutating command.
pub fn format(request: &ControlRequest, value: &Value) -> String {
    let dash = |v: &Value| v.as_str().map_or_else(|| "-".to_string(), str::to_string);
    let list = |v: &Value| {
        let items: Vec<&str> = v
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if items.is_empty() {
            "-".to_string()
        } else {
            items.join(",")
        }
    };
    let mut out = String::new();
    match request {
        ControlRequest::List { .. } | ControlRequest::Status => {
            if matches!(request, ControlRequest::Status) {
                let server = &value["server"];
                out.push_str(&format!(
                    "server\t{}\t{}\n",
                    dash(&server["version"]),
                    dash(&server["mode"])
                ));
                let generation = &value["keeper"]["generation"];
                if !generation.is_null() {
                    let trial = if generation["on_trial"].as_bool() == Some(true) {
                        "trial"
                    } else {
                        "confirmed"
                    };
                    out.push_str(&format!(
                        "generation\t{}\t{}\t{trial}\n",
                        generation["pid"]
                            .as_u64()
                            .map_or_else(|| "-".to_string(), |pid| pid.to_string()),
                        dash(&generation["version"]),
                    ));
                }
                if let Some(keeper) = value["keeper"]["version"].as_str() {
                    out.push_str(&format!("keeper\t{keeper}\n"));
                }
                let cluster = &value["cluster"];
                if cluster.is_object() {
                    let role = if cluster["leader"].as_bool() == Some(true) {
                        "leader"
                    } else {
                        "follower"
                    };
                    out.push_str(&format!(
                        "cluster\t{}\t{role}\t{}\n",
                        dash(&cluster["pod"]),
                        dash(&cluster["leader_name"])
                    ));
                    for member in cluster["members"].as_array().into_iter().flatten() {
                        out.push_str(&format!(
                            "member\t{}\t{}\t{}\t{}\t{}\n",
                            dash(&member["pod"]),
                            if member["live"].as_bool() == Some(true) {
                                "live"
                            } else {
                                "stale"
                            },
                            member["serving"].as_u64().unwrap_or(0),
                            if member["ready"].as_bool() == Some(true) {
                                "ready"
                            } else {
                                "not-ready"
                            },
                            dash(&member["server"]),
                        ));
                    }
                }
            }
            for app in value["apps"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\n",
                    dash(&app["app"]),
                    dash(&app["kind"]),
                    dash(&app["active"]),
                    list(&app["kept"]),
                    dash(&app["pinned"]),
                    list(&app["failed"]),
                ));
            }
            for line in value["incompatible"].as_array().into_iter().flatten() {
                out.push_str(&format!("incompatible\t{}\n", dash(line)));
            }
        }
        ControlRequest::Check { .. } => {
            for app in value["check"].as_array().into_iter().flatten() {
                match app.get("error").and_then(Value::as_str) {
                    Some(error) => {
                        out.push_str(&format!("{}\terror: {error}\n", dash(&app["app"])))
                    }
                    None => out.push_str(&format!(
                        "{}\t{}\n",
                        dash(&app["app"]),
                        dash(&app["candidate"])
                    )),
                }
            }
        }
        _ => {
            out.push_str(&dash(value));
            out.push('\n');
        }
    }
    out
}
