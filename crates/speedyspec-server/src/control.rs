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
}

/// `Ok` carries either a line to print (mutating commands) or the data `list`, `check` and `status` format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ControlResponse {
    Ok(Value),
    Error(String),
}

/// Answers control requests. Called on a blocking thread.
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
        let result: Result<Value, String> = match request {
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
                listing(&self.server_version, "managed", rows, incompatible)
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

/// Where the control socket of a server with this `run_dir` lives.
pub fn socket_path(run_dir: &Path) -> PathBuf {
    run_dir.join("control.sock")
}

/// Binds the control socket (replacing a stale one), makes it owner-only, and serves it until the process ends.
/// With `create_dir` false (immutable mode, which never writes to disk) the socket's directory must already exist,
/// for example a tmpfs or an `emptyDir` volume.
pub fn serve(
    path: &Path,
    backend: Arc<dyn ControlBackend>,
    create_dir: bool,
) -> std::io::Result<()> {
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
    let listener = tokio::net::UnixListener::bind(&staging)?;
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&staging, path)?;
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let backend = backend.clone();
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
                let response = match serde_json::from_str::<ControlRequest>(&line) {
                    Ok(request) => tokio::task::spawn_blocking(move || backend.handle(request))
                        .await
                        .unwrap_or_else(|err| {
                            ControlResponse::Error(format!("the request failed: {err}"))
                        }),
                    Err(err) => ControlResponse::Error(format!("not a control request: {err}")),
                };
                let mut out = serde_json::to_string(&response).expect("responses serialize");
                out.push('\n');
                let _ = write.write_all(out.as_bytes()).await;
            });
        }
    });
    Ok(())
}

/// Sends one request to the server's control socket and waits for the answer.
pub fn request(path: &Path, request: &ControlRequest) -> Result<ControlResponse, String> {
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
    serde_json::from_str(&answer)
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
