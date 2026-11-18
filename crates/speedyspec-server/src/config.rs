//! The server's TOML configuration, parsed and checked.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use speedyspec_server_domain::{Access, AppId, AppKind, HostMatch, Mount, MountTable};

use crate::listeners::TlsFiles;

/// Where `run` looks for the configuration without `--config`.
pub const DEFAULT_CONFIG: &str = "/etc/speedyspec/server.toml";

/// The built-in mount of the server app: the admin API, for any host, signed-in only.
pub const ADMIN_API_PATH: &str = "/admin/api/";

const DEFAULT_RUN_DIR: &str = "/run/speedyspec";
const DEFAULT_STATIC_MEMORY: u64 = 512 * 1024 * 1024;

/// How releases reach the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Writable `data_dir`; releases are downloaded, switched and rolled back by the server.
    Managed,
    /// Everything baked into the image at fixed paths; the server never writes.
    Immutable,
}

/// Which access check gates `signed-in` mounts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccessImpl {
    Deny,
    DevAllowAll,
}

/// When a managed app checks for and installs new releases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdatePolicy {
    Manual,
    Auto,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListenerConfig {
    pub address: SocketAddr,
    pub tls: Option<TlsFiles>,
    pub http3: bool,
}

/// One app's settings. The release-management fields are read now and used by managed mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppConfig {
    pub kind: AppKind,
    /// Immutable mode: the release directory baked into the image.
    pub path: Option<PathBuf>,
    /// Managed mode: the release index URL.
    pub source: Option<String>,
    pub keys: Vec<String>,
    pub keep: Option<u32>,
    pub update: Option<UpdatePolicy>,
    pub check_interval: Option<String>,
    pub prerelease: bool,
    pub allow_unsigned: bool,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    pub data_dir: Option<PathBuf>,
    pub run_dir: PathBuf,
    pub listeners: Vec<ListenerConfig>,
    pub trusted_proxies: Vec<ipnet::IpNet>,
    pub apps: BTreeMap<AppId, AppConfig>,
    pub mounts: MountTable,
    pub access: AccessImpl,
    pub static_memory: u64,
}

/// A configuration that could not be read or breaks a rule. Every message names the key and value.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("reading {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("config is not valid TOML for this server: {0}")]
    Parse(String),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default = "managed")]
    mode: Mode,
    data_dir: Option<PathBuf>,
    run_dir: Option<PathBuf>,
    #[serde(default)]
    listener: Vec<RawListener>,
    #[serde(default)]
    proxy: RawProxy,
    #[serde(default)]
    app: BTreeMap<String, RawApp>,
    #[serde(default)]
    mount: Vec<RawMount>,
    #[serde(default)]
    access: RawAccess,
    #[serde(default)]
    limits: RawLimits,
}

fn managed() -> Mode {
    Mode::Managed
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawListener {
    address: SocketAddr,
    tls: Option<RawTls>,
    #[serde(default)]
    http3: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTls {
    cert: PathBuf,
    key: PathBuf,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProxy {
    #[serde(default)]
    trusted: Vec<ipnet::IpNet>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawApp {
    kind: AppKind,
    path: Option<PathBuf>,
    source: Option<String>,
    #[serde(default)]
    keys: Vec<String>,
    keep: Option<u32>,
    update: Option<UpdatePolicy>,
    check_interval: Option<String>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    allow_unsigned: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMount {
    #[serde(default = "any_host")]
    host: String,
    path: String,
    app: String,
    #[serde(default = "public")]
    access: Access,
}

fn any_host() -> String {
    "*".to_string()
}

fn public() -> Access {
    Access::Public
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAccess {
    #[serde(default = "deny")]
    implementation: AccessImpl,
}

impl Default for RawAccess {
    fn default() -> Self {
        RawAccess {
            implementation: AccessImpl::Deny,
        }
    }
}

fn deny() -> AccessImpl {
    AccessImpl::Deny
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLimits {
    #[serde(default = "static_memory")]
    static_memory: u64,
}

impl Default for RawLimits {
    fn default() -> Self {
        RawLimits {
            static_memory: DEFAULT_STATIC_MEMORY,
        }
    }
}

fn static_memory() -> u64 {
    DEFAULT_STATIC_MEMORY
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::Invalid(message.into())
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| ConfigError::Io(path.to_path_buf(), err))?;
        Config::parse(&text)
    }

    pub fn parse(toml: &str) -> Result<Config, ConfigError> {
        let raw: RawConfig =
            toml::from_str(toml).map_err(|err| ConfigError::Parse(err.to_string()))?;

        if raw.listener.is_empty() {
            return Err(invalid("at least one [[listener]] is required"));
        }
        let mut listeners = Vec::with_capacity(raw.listener.len());
        for listener in raw.listener {
            if listener.http3 && listener.tls.is_none() {
                return Err(invalid(format!(
                    "listener {}: http3 = true needs tls",
                    listener.address
                )));
            }
            listeners.push(ListenerConfig {
                address: listener.address,
                tls: listener.tls.map(|tls| TlsFiles {
                    cert: tls.cert,
                    key: tls.key,
                }),
                http3: listener.http3,
            });
        }
        if raw.access.implementation == AccessImpl::DevAllowAll
            && let Some(listener) = listeners.iter().find(|l| !l.address.ip().is_loopback())
        {
            return Err(invalid(format!(
                "access.implementation = \"dev-allow-all\" needs every listener on a loopback address, but {} is not loopback",
                listener.address
            )));
        }

        let mut apps = BTreeMap::new();
        for (name, app) in raw.app {
            let id = AppId::parse(&name).map_err(|err| invalid(format!("[app.{name}]: {err}")))?;
            check_app(raw.mode, &id, &app)?;
            apps.insert(
                id,
                AppConfig {
                    kind: app.kind,
                    path: app.path,
                    source: app.source,
                    keys: app.keys,
                    keep: app.keep,
                    update: app.update,
                    check_interval: app.check_interval,
                    prerelease: app.prerelease,
                    allow_unsigned: app.allow_unsigned,
                },
            );
        }
        let servers: Vec<&AppId> = apps
            .iter()
            .filter(|(_, app)| app.kind == AppKind::Server)
            .map(|(id, _)| id)
            .collect();
        let [server] = servers.as_slice() else {
            return Err(invalid(format!(
                "exactly one app must have kind = \"server\", found {}",
                servers.len()
            )));
        };
        let server = (*server).clone();
        if raw.mode == Mode::Managed && raw.data_dir.is_none() {
            return Err(invalid("mode = \"managed\" needs data_dir"));
        }

        let mut mounts = Vec::with_capacity(raw.mount.len() + 1);
        for mount in raw.mount {
            let app = AppId::parse(&mount.app)
                .map_err(|err| invalid(format!("mount.app {:?}: {err}", mount.app)))?;
            match apps.get(&app) {
                None => {
                    return Err(invalid(format!(
                        "mount.app {:?} is not a configured app",
                        mount.app
                    )));
                }
                Some(config) if config.kind == AppKind::Server => {
                    return Err(invalid(format!(
                        "mount.app {:?} is the server app, whose only mount is the built-in {ADMIN_API_PATH}",
                        mount.app
                    )));
                }
                Some(_) => {}
            }
            if mount.host == "*" && mount.path == ADMIN_API_PATH {
                return Err(invalid(format!(
                    "mount.path {ADMIN_API_PATH:?} on host * is reserved for the admin API"
                )));
            }
            let host = if mount.host == "*" {
                HostMatch::Any
            } else {
                HostMatch::Exact(mount.host)
            };
            mounts.push(Mount {
                host,
                path: mount.path,
                app,
                access: mount.access,
            });
        }
        mounts.push(Mount {
            host: HostMatch::Any,
            path: ADMIN_API_PATH.to_string(),
            app: server,
            access: Access::SignedIn,
        });
        let mounts = MountTable::new(mounts).map_err(|err| invalid(format!("mount: {err}")))?;

        Ok(Config {
            mode: raw.mode,
            data_dir: raw.data_dir,
            run_dir: raw
                .run_dir
                .unwrap_or_else(|| PathBuf::from(DEFAULT_RUN_DIR)),
            listeners,
            trusted_proxies: raw.proxy.trusted,
            apps,
            mounts,
            access: raw.access.implementation,
            static_memory: raw.limits.static_memory,
        })
    }
}

/// The per-app rules: a mode mixes nothing, and the server app has neither `path` nor (until managed mode) anything
/// to mount.
fn check_app(mode: Mode, id: &AppId, app: &RawApp) -> Result<(), ConfigError> {
    if app.kind == AppKind::Server {
        if app.path.is_some() {
            return Err(invalid(format!(
                "app.{id}.path: the server app's release is the binary itself"
            )));
        }
        return Ok(());
    }
    match mode {
        Mode::Immutable => {
            if app.source.is_some() {
                return Err(invalid(format!(
                    "app.{id}.source is not allowed with mode = \"immutable\"; use path"
                )));
            }
            if app.path.is_none() {
                return Err(invalid(format!(
                    "app.{id}.path is required with mode = \"immutable\""
                )));
            }
        }
        Mode::Managed => {
            if app.path.is_some() {
                return Err(invalid(format!(
                    "app.{id}.path is not allowed with mode = \"managed\"; use source"
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC_EXAMPLE: &str = r#"
data_dir = "/var/lib/speedyspec"

[[listener]]
address = "127.0.0.1:8080"

[[listener]]
address = "0.0.0.0:443"
tls = { cert = "/etc/speedyspec/tls/fullchain.pem", key = "/etc/speedyspec/tls/key.pem" }
http3 = true

[proxy]
trusted = ["127.0.0.1/32", "10.0.0.0/8"]

[app.server]
kind = "server"
source = "https://releases.example.com/server/index.json"
keys = ["RWQ..."]
keep = 3
update = "manual"

[app.admin]
kind = "static"
source = "https://releases.example.com/admin/index.json"
keys = ["RWQ..."]
keep = 3
update = "auto"

[[mount]]
host = "*"
path = "/admin/"
app = "admin"
access = "signed-in"
"#;

    const IMMUTABLE: &str = r#"
mode = "immutable"

[[listener]]
address = "127.0.0.1:8080"

[app.server]
kind = "server"

[app.admin]
kind = "static"
path = "/srv/admin"

[app.blog]
kind = "php"
path = "/srv/blog"

[[mount]]
path = "/admin/"
app = "admin"
access = "signed-in"

[[mount]]
host = "example.com"
path = "/"
app = "blog"
"#;

    fn id(value: &str) -> AppId {
        AppId::parse(value).unwrap()
    }

    fn rejects(toml: &str, needle: &str) {
        match Config::parse(toml) {
            Ok(_) => panic!("must be rejected (expected a message containing {needle:?}):\n{toml}"),
            Err(err) => {
                let message = err.to_string();
                assert!(
                    message.contains(needle),
                    "message {message:?} must contain {needle:?}"
                );
            }
        }
    }

    #[test]
    fn parses_spec_example() {
        let config = Config::parse(SPEC_EXAMPLE).unwrap();
        assert_eq!(config.mode, Mode::Managed);
        assert_eq!(
            config.data_dir.as_deref(),
            Some(std::path::Path::new("/var/lib/speedyspec"))
        );
        assert_eq!(config.run_dir, std::path::PathBuf::from("/run/speedyspec"));
        assert_eq!(config.listeners.len(), 2);
        assert!(config.listeners[1].http3);
        assert_eq!(
            config.listeners[1].tls.as_ref().unwrap().cert,
            std::path::PathBuf::from("/etc/speedyspec/tls/fullchain.pem")
        );
        assert_eq!(config.trusted_proxies.len(), 2);
        assert_eq!(config.apps[&id("server")].kind, AppKind::Server);
        assert_eq!(
            config.apps[&id("admin")].source.as_deref(),
            Some("https://releases.example.com/admin/index.json")
        );
        assert_eq!(config.access, AccessImpl::Deny);
        assert_eq!(config.static_memory, speedyspec_server_static_limit());
        let admin = config.mounts.resolve("example.com", "/admin/x").unwrap();
        assert_eq!(
            (admin.app.as_str(), admin.access),
            ("admin", Access::SignedIn)
        );
        let api = config
            .mounts
            .resolve("example.com", "/admin/api/x")
            .unwrap();
        assert_eq!((api.app.as_str(), api.access), ("server", Access::SignedIn));
    }

    fn speedyspec_server_static_limit() -> u64 {
        512 * 1024 * 1024
    }

    #[test]
    fn parses_immutable_example() {
        let config = Config::parse(IMMUTABLE).unwrap();
        assert_eq!(config.mode, Mode::Immutable);
        assert_eq!(config.data_dir, None);
        assert_eq!(config.apps[&id("blog")].kind, AppKind::Php);
        assert_eq!(
            config.apps[&id("admin")].path.as_deref(),
            Some(std::path::Path::new("/srv/admin"))
        );
        assert_eq!(
            config
                .mounts
                .resolve("example.com", "/x")
                .unwrap()
                .app
                .as_str(),
            "blog"
        );
        assert_eq!(
            config
                .mounts
                .resolve("other.org", "/admin/")
                .unwrap()
                .access,
            Access::SignedIn
        );
        assert_eq!(config.mounts.resolve("other.org", "/x"), None);
    }

    #[test]
    fn parses_access_and_limits() {
        let toml = format!(
            "{IMMUTABLE}\n[access]\nimplementation = \"dev-allow-all\"\n\n[limits]\nstatic_memory = 1024\n"
        );
        let config = Config::parse(&toml).unwrap();
        assert_eq!(config.access, AccessImpl::DevAllowAll);
        assert_eq!(config.static_memory, 1024);
    }

    #[test]
    fn rejects_two_server_apps() {
        rejects(
            &IMMUTABLE.replace(
                "[app.blog]\nkind = \"php\"\npath = \"/srv/blog\"",
                "[app.blog]\nkind = \"server\"",
            ),
            "kind = \"server\"",
        );
    }

    #[test]
    fn rejects_no_server_app() {
        rejects(
            &IMMUTABLE.replace("[app.server]\nkind = \"server\"\n", ""),
            "kind = \"server\"",
        );
    }

    #[test]
    fn rejects_server_with_path() {
        rejects(
            &IMMUTABLE.replace(
                "[app.server]\nkind = \"server\"",
                "[app.server]\nkind = \"server\"\npath = \"/srv/x\"",
            ),
            "app.server.path",
        );
    }

    #[test]
    fn rejects_mount_on_server_app() {
        rejects(
            &format!("{IMMUTABLE}\n[[mount]]\npath = \"/srv/\"\napp = \"server\"\n"),
            "mount.app",
        );
    }

    #[test]
    fn rejects_immutable_with_source() {
        rejects(
            &IMMUTABLE.replace(
                "path = \"/srv/admin\"",
                "path = \"/srv/admin\"\nsource = \"https://x\"",
            ),
            "app.admin.source",
        );
    }

    #[test]
    fn rejects_immutable_without_path() {
        rejects(
            &IMMUTABLE.replace("path = \"/srv/admin\"\n", ""),
            "app.admin.path",
        );
    }

    #[test]
    fn rejects_managed_with_path() {
        rejects(
            &SPEC_EXAMPLE.replace(
                "update = \"auto\"",
                "update = \"auto\"\npath = \"/srv/admin\"",
            ),
            "app.admin.path",
        );
    }

    #[test]
    fn rejects_managed_without_data_dir() {
        rejects(
            &SPEC_EXAMPLE.replace("data_dir = \"/var/lib/speedyspec\"", ""),
            "data_dir",
        );
    }

    #[test]
    fn rejects_unknown_mount_app() {
        rejects(
            &format!("{IMMUTABLE}\n[[mount]]\npath = \"/x/\"\napp = \"nope\"\n"),
            "nope",
        );
    }

    #[test]
    fn rejects_admin_api_duplicate() {
        rejects(
            &format!("{IMMUTABLE}\n[[mount]]\npath = \"/admin/api/\"\napp = \"blog\"\n"),
            "/admin/api/",
        );
    }

    #[test]
    fn rejects_bad_mount_path() {
        rejects(
            &IMMUTABLE.replace("path = \"/admin/\"", "path = \"/admin\""),
            "/admin",
        );
    }

    #[test]
    fn rejects_http3_without_tls() {
        rejects(
            &IMMUTABLE.replace(
                "address = \"127.0.0.1:8080\"",
                "address = \"127.0.0.1:8080\"\nhttp3 = true",
            ),
            "http3",
        );
    }

    #[test]
    fn rejects_dev_allow_all_on_public_address() {
        let toml = SPEC_EXAMPLE.replace(
            "[proxy]",
            "[access]\nimplementation = \"dev-allow-all\"\n\n[proxy]",
        );
        rejects(&toml, "0.0.0.0:443");
        rejects(&toml, "loopback");
    }

    #[test]
    fn rejects_no_listeners() {
        rejects(
            &IMMUTABLE.replace("[[listener]]\naddress = \"127.0.0.1:8080\"\n", ""),
            "listener",
        );
    }

    #[test]
    fn rejects_unknown_key() {
        rejects(&format!("colour = \"blue\"\n{IMMUTABLE}"), "colour");
    }

    #[test]
    fn rejects_bad_update_policy() {
        rejects(
            &SPEC_EXAMPLE.replace("update = \"auto\"", "update = \"sometimes\""),
            "sometimes",
        );
    }
}
