//! The server's TOML configuration, parsed and checked.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use speedyspec_server_domain::{Access, AppId, AppKind, HostMatch, Mount, MountTable};

use crate::listeners::TlsFiles;
use speedyspec_server_presentation::ForwardedHeaders;

/// Where `run` looks for the configuration without `--config`.
pub const DEFAULT_CONFIG: &str = "/etc/speedyspec/server.toml";

/// The built-in mount of the server app: the admin API, for any host, signed-in only.

const DEFAULT_RUN_DIR: &str = "/run/speedyspec";
const DEFAULT_STATIC_MEMORY: u64 = 512 * 1024 * 1024;
const DEFAULT_KEEP: u32 = 3;
const DEFAULT_CHECK_INTERVAL: Duration = Duration::from_secs(15 * 60);
const DEFAULT_FALLBACK_GRACE: Duration = Duration::from_secs(60 * 60);

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
    /// The admin gate: release access rules and the gate app's answers (`[gate]`).
    Gate,
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
    /// Kept releases, at least 1 (default 3).
    pub keep: u32,
    /// Default `manual` for the server, `auto` for other apps.
    pub update: UpdatePolicy,
    /// How often an `auto` app checks for releases (default 15 minutes).
    pub check_interval: Duration,
    pub prerelease: bool,
    pub allow_unsigned: bool,
    /// The worker pool of a `php` app (`app.<id>.php`); `None` for other kinds.
    pub php: Option<PhpConfig>,
    /// The `runtime` app a `php` app's workers run (`app.<id>.runtime`, or the only runtime app); `None` for other
    /// kinds.
    pub runtime: Option<AppId>,
    /// The static app whose maintenance page this app shows while in maintenance (`app.<id>.maintenance_page`).
    pub maintenance_page: Option<AppId>,
}

/// `[health]`: how often and how patiently each PHP app's health probes are called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealthConfig {
    pub interval: Duration,
    pub timeout: Duration,
    /// Failed rounds in a row that turn maintenance on.
    pub fail_after: u32,
    /// Passed rounds in a row that turn it off again.
    pub pass_after: u32,
}

pub const DEFAULT_HEALTH: HealthConfig = HealthConfig {
    interval: Duration::from_secs(2),
    timeout: Duration::from_secs(1),
    fail_after: 3,
    pass_after: 3,
};

/// How many workers a php app's pool keeps: `min` always, up to `max` on demand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerRange {
    pub min: u32,
    pub max: u32,
}

/// `app.<id>.php`: a php app's worker pool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhpConfig {
    pub workers: WorkerRange,
    /// Requests before a worker is recycled.
    pub max_requests: u64,
    /// A request running longer is killed with its worker (504).
    pub request_timeout: Duration,
    /// A worker whose memory passes this after a request is recycled, in bytes.
    pub memory_limit: u64,
    /// An idle worker above `min` exits after this.
    pub idle_timeout: Duration,
    /// The app's writable directory (uploads, `wp-content`, `.env`); required in managed mode.
    pub site_dir: Option<PathBuf>,
    /// `(port key, PHP class)` bindings installed at the start of every request.
    pub bindings: Vec<(String, String)>,
    /// `app.<id>.php.env`: environment variables for the app's workers.
    pub env: Vec<(String, String)>,
    /// `app.<id>.php.env_file`: a file of `KEY=value` lines for the app's workers.
    pub env_file: Option<PathBuf>,
}

impl PhpConfig {
    /// The app's worker environment: the site directory's `.env`, then `env_file`, then `env`, a later source winning
    /// for a key. Read when a pool starts, so a changed file applies to the next release switch. A missing `env_file`
    /// is an error; a missing `.env` is not.
    pub fn worker_env(&self) -> Result<Vec<(String, String)>, String> {
        let mut env = BTreeMap::new();
        if let Some(site) = &self.site_dir
            && let Ok(text) = std::fs::read_to_string(site.join(".env"))
        {
            env.extend(env_lines(&text));
        }
        if let Some(file) = &self.env_file {
            let text = std::fs::read_to_string(file)
                .map_err(|err| format!("reading env_file {}: {err}", file.display()))?;
            env.extend(env_lines(&text));
        }
        env.extend(self.env.iter().cloned());
        Ok(env.into_iter().collect())
    }
}

/// `KEY=value` lines, nothing evaluated; comments and blank lines skipped.
fn env_lines(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

pub const DEFAULT_PHP_WORKERS: WorkerRange = WorkerRange { min: 1, max: 4 };
pub const DEFAULT_PHP_MAX_REQUESTS: u64 = 1000;
pub const DEFAULT_PHP_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
pub const DEFAULT_PHP_MEMORY_LIMIT: u64 = 256 * 1024 * 1024;
pub const DEFAULT_PHP_IDLE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    pub data_dir: Option<PathBuf>,
    pub run_dir: PathBuf,
    pub listeners: Vec<ListenerConfig>,
    pub trusted_proxies: Vec<ipnet::IpNet>,
    /// Which proxy headers the trusted proxies set (`proxy.headers`, default `x-forwarded`).
    pub forwarded_headers: ForwardedHeaders,
    pub apps: BTreeMap<AppId, AppConfig>,
    pub mounts: MountTable,
    pub access: AccessImpl,
    pub static_memory: u64,
    /// How long a static app keeps serving the previous release's immutable files after a switch
    /// (`[release.static] fallback_grace`, default 1 hour).
    pub static_fallback_grace: Duration,
    pub handoff: HandoffConfig,
    /// `[cluster]`; `None` is a single host.
    pub cluster: Option<ClusterConfig>,
    /// `[health]`.
    pub health: HealthConfig,
    /// `[gate]`, required with `access.implementation = "gate"`.
    pub gate: Option<speedyspec_server_gate::GateSettings>,
    /// `[switch] migrate_timeout`: how long a release's migration may run (default 30 minutes).
    pub migrate_timeout: Duration,
}

/// Who decides which pod leads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Leadership {
    /// One host, always the leader.
    Single,
    /// A `coordination.k8s.io/v1` Lease.
    Kubernetes,
    /// A lease file on one host, for the multi-process tests (only with the `test-hooks` feature).
    File,
}

/// `[cluster]`: several pods on one shared `data_dir`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClusterConfig {
    pub leadership: Leadership,
    /// This pod's name (default `$POD_NAME`, else the host name).
    pub pod: String,
    pub lease_name: String,
    /// Default: the service account's namespace file, read when the Lease starts.
    pub lease_namespace: Option<String>,
    pub lease_duration: Duration,
    pub renew_deadline: Duration,
    pub retry_period: Duration,
    pub poll_interval: Duration,
    pub prepare_timeout: Duration,
    /// How long one pod's rollout turn may take before the rollout fails (default 10 minutes).
    pub turn_timeout: Duration,
    /// Where `/readyz` and `/livez` are served, if anywhere.
    pub probe: Option<SocketAddr>,
}

/// Socket handoff timing (`[handoff]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoffConfig {
    /// How long a new generation has to report ready (default 30 s).
    pub ready_timeout: Duration,
    /// How long a draining generation keeps open connections (default 5 minutes).
    pub drain_timeout: Duration,
    /// How long a new server version is on trial (default 2 minutes).
    pub trial_period: Duration,
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
    #[serde(default)]
    release: RawRelease,
    #[serde(default)]
    handoff: RawHandoff,
    cluster: Option<RawCluster>,
    #[serde(default)]
    health: RawHealth,
    gate: Option<RawGate>,
    #[serde(default, rename = "switch")]
    switch_: RawSwitch,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawSwitch {
    migrate_timeout: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGate {
    app: Option<String>,
    session_ttl: Option<String>,
    timeout: Option<String>,
    site_ttl: Option<String>,
    poll: Option<String>,
    max_pending: Option<usize>,
    maintenance_apps: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCluster {
    #[serde(default = "single")]
    leadership: Leadership,
    pod: Option<String>,
    lease_name: Option<String>,
    lease_namespace: Option<String>,
    lease_duration: Option<String>,
    renew_deadline: Option<String>,
    retry_period: Option<String>,
    poll_interval: Option<String>,
    prepare_timeout: Option<String>,
    turn_timeout: Option<String>,
    probe: Option<SocketAddr>,
}

fn single() -> Leadership {
    Leadership::Single
}

/// `$POD_NAME`, else the host name.
fn default_pod() -> String {
    std::env::var("POD_NAME")
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            rustix::system::uname()
                .nodename()
                .to_string_lossy()
                .into_owned()
        })
}

fn cluster_config(mode: Mode, raw: RawCluster) -> Result<ClusterConfig, ConfigError> {
    if mode != Mode::Managed {
        return Err(invalid(
            "[cluster] needs mode = \"managed\": an immutable image has no releases to coordinate",
        ));
    }
    if raw.leadership == Leadership::File && !cfg!(feature = "test-hooks") {
        return Err(invalid(
            "cluster.leadership = \"file\" exists only in builds with the test-hooks feature",
        ));
    }
    let lease_duration = duration(
        "cluster.lease_duration",
        raw.lease_duration.as_deref(),
        Duration::from_secs(15),
    )?;
    let renew_deadline = duration(
        "cluster.renew_deadline",
        raw.renew_deadline.as_deref(),
        Duration::from_secs(10),
    )?;
    let retry_period = duration(
        "cluster.retry_period",
        raw.retry_period.as_deref(),
        Duration::from_secs(2),
    )?;
    if renew_deadline >= lease_duration {
        return Err(invalid(format!(
            "cluster.renew_deadline ({}) must be below cluster.lease_duration ({}), so a leader stops writing before anyone else can take the lease",
            humantime::format_duration(renew_deadline),
            humantime::format_duration(lease_duration)
        )));
    }
    if retry_period >= renew_deadline {
        return Err(invalid(format!(
            "cluster.retry_period ({}) must be below cluster.renew_deadline ({})",
            humantime::format_duration(retry_period),
            humantime::format_duration(renew_deadline)
        )));
    }
    Ok(ClusterConfig {
        leadership: raw.leadership,
        pod: raw
            .pod
            .filter(|pod| !pod.is_empty())
            .unwrap_or_else(default_pod),
        lease_name: raw
            .lease_name
            .unwrap_or_else(|| "speedyspec-server".to_string()),
        lease_namespace: raw.lease_namespace,
        lease_duration,
        renew_deadline,
        retry_period,
        poll_interval: duration(
            "cluster.poll_interval",
            raw.poll_interval.as_deref(),
            Duration::from_secs(2),
        )?,
        prepare_timeout: duration(
            "cluster.prepare_timeout",
            raw.prepare_timeout.as_deref(),
            Duration::from_secs(60),
        )?,
        turn_timeout: duration(
            "cluster.turn_timeout",
            raw.turn_timeout.as_deref(),
            Duration::from_secs(600),
        )?,
        probe: raw.probe,
    })
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHandoff {
    ready_timeout: Option<String>,
    drain_timeout: Option<String>,
    trial_period: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRelease {
    #[serde(rename = "static", default)]
    static_: RawReleaseStatic,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReleaseStatic {
    fallback_grace: Option<String>,
}

/// A humantime duration (`15m`, `1h`, `30s`), or `default` when absent. The error names the key and the value.
fn duration(key: &str, value: Option<&str>, default: Duration) -> Result<Duration, ConfigError> {
    match value {
        None => Ok(default),
        Some(text) => humantime::parse_duration(text).map_err(|err| {
            invalid(format!(
                "{key} = {text:?} is not a duration like \"15m\" or \"1h\": {err}"
            ))
        }),
    }
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
    #[serde(default)]
    headers: RawForwardedHeaders,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RawForwardedHeaders {
    #[default]
    XForwarded,
    Forwarded,
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
    php: Option<RawPhp>,
    runtime: Option<String>,
    maintenance_page: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawHealth {
    interval: Option<String>,
    timeout: Option<String>,
    fail_after: Option<u32>,
    pass_after: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawPhp {
    workers: Option<RawWorkers>,
    max_requests: Option<u64>,
    request_timeout: Option<String>,
    memory_limit: Option<RawBytes>,
    idle_timeout: Option<String>,
    site_dir: Option<PathBuf>,
    #[serde(default)]
    bindings: BTreeMap<String, String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    env_file: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkers {
    min: u32,
    max: u32,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawBytes {
    Count(u64),
    Text(String),
}

/// A size: bytes, or a number with K, M, G (or KiB, MiB, GiB), all powers of 1024.
fn bytes(key: &str, value: &RawBytes) -> Result<u64, ConfigError> {
    let text = match value {
        RawBytes::Count(n) => return Ok(*n),
        RawBytes::Text(text) => text.trim(),
    };
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, unit) = text.split_at(split);
    let scale: u64 = match unit.trim() {
        "" | "B" => 1,
        "K" | "KiB" => 1024,
        "M" | "MiB" => 1024 * 1024,
        "G" | "GiB" => 1024 * 1024 * 1024,
        _ => 0,
    };
    match digits
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(scale))
        .filter(|_| scale > 0)
    {
        Some(n) => Ok(n),
        None => Err(invalid(format!(
            "{key} = {text:?} is not a size like \"256MiB\""
        ))),
    }
}

fn php_config(mode: Mode, id: &AppId, raw: Option<RawPhp>) -> Result<PhpConfig, ConfigError> {
    let raw = raw.unwrap_or_default();
    let key = |field: &str| format!("app.{id}.php.{field}");
    let workers = match raw.workers {
        None => DEFAULT_PHP_WORKERS,
        Some(RawWorkers { min, max }) => {
            if max == 0 || min > max {
                return Err(invalid(format!(
                    "{} = {{ min = {min}, max = {max} }}: max must be at least 1 and at least min",
                    key("workers")
                )));
            }
            WorkerRange { min, max }
        }
    };
    let max_requests = raw.max_requests.unwrap_or(DEFAULT_PHP_MAX_REQUESTS);
    if max_requests == 0 {
        return Err(invalid(format!(
            "{} must be at least 1",
            key("max_requests")
        )));
    }
    let memory_limit = match &raw.memory_limit {
        None => DEFAULT_PHP_MEMORY_LIMIT,
        Some(value) => bytes(&key("memory_limit"), value)?,
    };
    if mode == Mode::Managed && raw.site_dir.is_none() {
        return Err(invalid(format!(
            "{} is required with mode = \"managed\": it is the app's writable directory",
            key("site_dir")
        )));
    }
    Ok(PhpConfig {
        workers,
        max_requests,
        request_timeout: duration(
            &key("request_timeout"),
            raw.request_timeout.as_deref(),
            DEFAULT_PHP_REQUEST_TIMEOUT,
        )?,
        memory_limit,
        idle_timeout: duration(
            &key("idle_timeout"),
            raw.idle_timeout.as_deref(),
            DEFAULT_PHP_IDLE_TIMEOUT,
        )?,
        site_dir: raw.site_dir,
        bindings: raw.bindings.into_iter().collect(),
        env: raw.env.into_iter().collect(),
        env_file: raw.env_file,
    })
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
            if app.keep == Some(0) {
                return Err(invalid(format!("app.{id}.keep must be at least 1")));
            }
            let check_interval = duration(
                &format!("app.{id}.check_interval"),
                app.check_interval.as_deref(),
                DEFAULT_CHECK_INTERVAL,
            )?;
            let default_update = if app.kind == AppKind::Server {
                UpdatePolicy::Manual
            } else {
                UpdatePolicy::Auto
            };
            if app.kind != AppKind::Php && app.php.is_some() {
                return Err(invalid(format!(
                    "app.{id}.php is only for apps with kind = \"php\""
                )));
            }
            let php = match app.kind {
                AppKind::Php => Some(php_config(raw.mode, &id, app.php)?),
                _ => None,
            };
            let maintenance_page = app
                .maintenance_page
                .as_deref()
                .map(|name| {
                    AppId::parse(name).map_err(|err| {
                        invalid(format!("app.{id}.maintenance_page {name:?}: {err}"))
                    })
                })
                .transpose()?;
            let runtime = match (&app.runtime, app.kind) {
                (None, _) => None,
                (Some(name), AppKind::Php) => Some(
                    AppId::parse(name)
                        .map_err(|err| invalid(format!("app.{id}.runtime {name:?}: {err}")))?,
                ),
                (Some(_), _) => {
                    return Err(invalid(format!(
                        "app.{id}.runtime is only for apps with kind = \"php\""
                    )));
                }
            };
            apps.insert(
                id,
                AppConfig {
                    kind: app.kind,
                    php,
                    path: app.path,
                    source: app.source,
                    keys: app.keys,
                    keep: app.keep.unwrap_or(DEFAULT_KEEP),
                    update: app.update.unwrap_or(default_update),
                    check_interval,
                    prerelease: app.prerelease,
                    allow_unsigned: app.allow_unsigned,
                    runtime,
                    maintenance_page,
                },
            );
        }
        resolve_runtimes(&mut apps)?;
        for (id, app) in &apps {
            if let Some(page) = &app.maintenance_page
                && apps
                    .get(page)
                    .is_none_or(|other| other.kind != AppKind::Static)
            {
                return Err(invalid(format!(
                    "app.{id}.maintenance_page {page:?} is not a configured app with kind = \"static\""
                )));
            }
        }
        let gate = match raw.gate {
            None if raw.access.implementation == AccessImpl::Gate => {
                return Err(invalid(
                    "access.implementation = \"gate\" needs [gate] with gate.app naming the admin API app",
                ));
            }
            None => None,
            Some(raw_gate) => Some(gate_settings(raw_gate, &apps)?),
        };
        let health = HealthConfig {
            interval: duration(
                "health.interval",
                raw.health.interval.as_deref(),
                DEFAULT_HEALTH.interval,
            )?,
            timeout: duration(
                "health.timeout",
                raw.health.timeout.as_deref(),
                DEFAULT_HEALTH.timeout,
            )?,
            fail_after: raw
                .health
                .fail_after
                .unwrap_or(DEFAULT_HEALTH.fail_after)
                .max(1),
            pass_after: raw
                .health
                .pass_after
                .unwrap_or(DEFAULT_HEALTH.pass_after)
                .max(1),
        };
        let servers: Vec<&AppId> = apps
            .iter()
            .filter(|(_, app)| app.kind == AppKind::Server)
            .map(|(id, _)| id)
            .collect();
        if servers.len() != 1 {
            return Err(invalid(format!(
                "exactly one app must have kind = \"server\", found {}",
                servers.len()
            )));
        }
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
                        "mount.app {:?} is the server app, which has no mounts",
                        mount.app
                    )));
                }
                Some(_) => {}
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
        let static_fallback_grace = duration(
            "release.static.fallback_grace",
            raw.release.static_.fallback_grace.as_deref(),
            DEFAULT_FALLBACK_GRACE,
        )?;
        let handoff_duration = |key: &str,
                                value: &Option<String>,
                                default: Duration|
         -> Result<Duration, ConfigError> {
            let found = duration(key, value.as_deref(), default)?;
            if found < Duration::from_secs(1) {
                return Err(invalid(format!("{key} must be at least 1s")));
            }
            Ok(found)
        };
        let handoff = HandoffConfig {
            ready_timeout: handoff_duration(
                "handoff.ready_timeout",
                &raw.handoff.ready_timeout,
                Duration::from_secs(30),
            )?,
            drain_timeout: handoff_duration(
                "handoff.drain_timeout",
                &raw.handoff.drain_timeout,
                Duration::from_secs(300),
            )?,
            trial_period: handoff_duration(
                "handoff.trial_period",
                &raw.handoff.trial_period,
                Duration::from_secs(120),
            )?,
        };
        let mounts = MountTable::new(mounts).map_err(|err| invalid(format!("mount: {err}")))?;

        Ok(Config {
            mode: raw.mode,
            data_dir: raw.data_dir,
            run_dir: raw
                .run_dir
                .unwrap_or_else(|| PathBuf::from(DEFAULT_RUN_DIR)),
            listeners,
            trusted_proxies: raw.proxy.trusted,
            forwarded_headers: match raw.proxy.headers {
                RawForwardedHeaders::XForwarded => ForwardedHeaders::XForwarded,
                RawForwardedHeaders::Forwarded => ForwardedHeaders::Forwarded,
            },
            apps,
            mounts,
            access: raw.access.implementation,
            static_memory: raw.limits.static_memory,
            static_fallback_grace,
            handoff,
            health,
            gate,
            migrate_timeout: duration(
                "switch.migrate_timeout",
                raw.switch_.migrate_timeout.as_deref(),
                Duration::from_secs(30 * 60),
            )?,
            cluster: raw
                .cluster
                .map(|c| cluster_config(raw.mode, c))
                .transpose()?,
        })
    }
}

/// The per-app rules: a mode mixes nothing, and the server app has neither `path` nor (until managed mode) anything
/// to mount.
/// `[gate]`: its app must be a configured php app.
fn gate_settings(
    raw: RawGate,
    apps: &BTreeMap<AppId, AppConfig>,
) -> Result<speedyspec_server_gate::GateSettings, ConfigError> {
    let name = raw
        .app
        .ok_or_else(|| invalid("gate.app is required: the admin API app the gate asks"))?;
    let app = AppId::parse(&name).map_err(|err| invalid(format!("gate.app {name:?}: {err}")))?;
    if apps
        .get(&app)
        .is_none_or(|config| config.kind != AppKind::Php)
    {
        return Err(invalid(format!(
            "gate.app {name:?} is not a configured app with kind = \"php\""
        )));
    }
    let maintenance_apps = raw
        .maintenance_apps
        .map(|names| {
            names
                .iter()
                .map(|name| match AppId::parse(name) {
                    Ok(id) if apps.contains_key(&id) => Ok(id),
                    _ => Err(invalid(format!(
                        "gate.maintenance_apps: {name:?} is not a configured app"
                    ))),
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    Ok(speedyspec_server_gate::GateSettings {
        app,
        session_ttl: duration(
            "gate.session_ttl",
            raw.session_ttl.as_deref(),
            Duration::from_secs(60),
        )?,
        timeout: duration(
            "gate.timeout",
            raw.timeout.as_deref(),
            Duration::from_secs(5),
        )?,
        site_ttl: duration(
            "gate.site_ttl",
            raw.site_ttl.as_deref(),
            Duration::from_secs(2),
        )?,
        poll: duration("gate.poll", raw.poll.as_deref(), Duration::from_secs(1))?,
        max_pending: raw.max_pending.unwrap_or(16).max(1),
        maintenance_apps,
    })
}

/// Gives every php app its runtime app: the one it names, which must be a `runtime` app, or the only runtime app
/// configured. A php app with several to choose from must name one.
fn resolve_runtimes(apps: &mut BTreeMap<AppId, AppConfig>) -> Result<(), ConfigError> {
    let runtimes: Vec<AppId> = apps
        .iter()
        .filter(|(_, app)| app.kind == AppKind::Runtime)
        .map(|(id, _)| id.clone())
        .collect();
    for (id, app) in apps.iter_mut().filter(|(_, app)| app.kind == AppKind::Php) {
        match (&app.runtime, runtimes.as_slice()) {
            (Some(named), _) if !runtimes.contains(named) => {
                return Err(invalid(format!(
                    "app.{id}.runtime {named:?} is not a configured app with kind = \"runtime\""
                )));
            }
            (Some(_), _) => {}
            (None, []) => {
                return Err(invalid(format!(
                    "app.{id}.runtime: a php app needs an app with kind = \"runtime\" to run on, and none is configured"
                )));
            }
            (None, [only]) => app.runtime = Some(only.clone()),
            (None, _) => {
                return Err(invalid(format!(
                    "app.{id}.runtime must name one of the runtime apps {}",
                    runtimes
                        .iter()
                        .map(AppId::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        }
    }
    Ok(())
}

impl Config {
    /// Each php app's runtime app.
    pub fn php_runtimes(&self) -> BTreeMap<AppId, AppId> {
        self.apps
            .iter()
            .filter_map(|(id, app)| app.runtime.clone().map(|runtime| (id.clone(), runtime)))
            .collect()
    }
}

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
            if app.source.is_none() {
                return Err(invalid(format!(
                    "app.{id}.source is required with mode = \"managed\": it is the release index URL"
                )));
            }
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

[app.rt]
kind = "runtime"
path = "/opt/rt/1.0.0"

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
        assert!(
            config
                .mounts
                .mounts()
                .iter()
                .all(|m| m.app.as_str() != "server")
        );
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
    fn forwarded_headers_family() {
        assert_eq!(
            Config::parse(IMMUTABLE).unwrap().forwarded_headers,
            ForwardedHeaders::XForwarded
        );
        let toml = format!("{IMMUTABLE}\n[proxy]\nheaders = \"forwarded\"\n");
        assert_eq!(
            Config::parse(&toml).unwrap().forwarded_headers,
            ForwardedHeaders::Forwarded
        );
        rejects(
            &format!("{IMMUTABLE}\n[proxy]\nheaders = \"both\"\n"),
            "both",
        );
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
    fn parses_release_keys_and_defaults() {
        let config = Config::parse(SPEC_EXAMPLE).unwrap();
        let admin = &config.apps[&id("admin")];
        assert_eq!(admin.keep, 3);
        assert_eq!(admin.update, UpdatePolicy::Auto);
        assert_eq!(
            admin.check_interval,
            std::time::Duration::from_secs(15 * 60)
        );
        assert_eq!(admin.keys, ["RWQ..."]);
        assert_eq!(config.apps[&id("server")].update, UpdatePolicy::Manual);
        assert_eq!(
            config.static_fallback_grace,
            std::time::Duration::from_secs(3600)
        );

        let bare = SPEC_EXAMPLE
            .replace("keep = 3\nupdate = \"auto\"", "")
            .replace("keep = 3\nupdate = \"manual\"", "");
        let config = Config::parse(&bare).unwrap();
        assert_eq!(config.apps[&id("admin")].keep, 3);
        assert_eq!(config.apps[&id("admin")].update, UpdatePolicy::Auto);
        assert_eq!(config.apps[&id("server")].update, UpdatePolicy::Manual);
    }

    #[test]
    fn rejects_keep_zero() {
        rejects(
            &SPEC_EXAMPLE.replace("keep = 3\nupdate = \"auto\"", "keep = 0\nupdate = \"auto\""),
            "app.admin.keep",
        );
    }

    #[test]
    fn rejects_bad_interval() {
        rejects(
            &SPEC_EXAMPLE.replace(
                "update = \"auto\"",
                "update = \"auto\"\ncheck_interval = \"often\"",
            ),
            "often",
        );
    }

    #[test]
    fn rejects_managed_without_source() {
        rejects(
            &SPEC_EXAMPLE.replace(
                "source = \"https://releases.example.com/admin/index.json\"\n",
                "",
            ),
            "app.admin.source",
        );
    }

    #[test]
    fn parses_fallback_grace() {
        let config = Config::parse(&format!(
            "{SPEC_EXAMPLE}\n[release.static]\nfallback_grace = \"10m\"\n"
        ))
        .unwrap();
        assert_eq!(
            config.static_fallback_grace,
            std::time::Duration::from_secs(600)
        );
        rejects(
            &format!("{SPEC_EXAMPLE}\n[release.static]\nfallback_grace = \"soon\"\n"),
            "soon",
        );
    }

    #[test]
    fn rejects_bad_update_policy() {
        rejects(
            &SPEC_EXAMPLE.replace("update = \"auto\"", "update = \"sometimes\""),
            "sometimes",
        );
    }

    #[test]
    fn handoff_defaults_and_overrides() {
        let config = Config::parse(IMMUTABLE).unwrap();
        assert_eq!(
            config.handoff.ready_timeout,
            std::time::Duration::from_secs(30)
        );
        assert_eq!(
            config.handoff.drain_timeout,
            std::time::Duration::from_secs(300)
        );
        assert_eq!(
            config.handoff.trial_period,
            std::time::Duration::from_secs(120)
        );
        let config =
            Config::parse(&format!("{IMMUTABLE}\n[handoff]\ntrial_period = \"3s\"\n")).unwrap();
        assert_eq!(
            config.handoff.trial_period,
            std::time::Duration::from_secs(3)
        );
        rejects(
            &format!("{IMMUTABLE}\n[handoff]\nready_timeout = \"10ms\"\n"),
            "handoff.ready_timeout",
        );
    }

    #[test]
    fn cluster_defaults() {
        assert!(
            Config::parse(SPEC_EXAMPLE).unwrap().cluster.is_none(),
            "no [cluster] is a single host"
        );
        let config = Config::parse(&format!(
            "{SPEC_EXAMPLE}\n[cluster]\nleadership = \"kubernetes\"\n"
        ))
        .unwrap();
        let cluster = config.cluster.expect("a cluster");
        assert_eq!(cluster.leadership, Leadership::Kubernetes);
        assert!(
            !cluster.pod.is_empty(),
            "the pod name defaults to $POD_NAME or the host name"
        );
        assert_eq!(cluster.lease_name, "speedyspec-server");
        assert_eq!(cluster.lease_namespace, None);
        assert_eq!(cluster.lease_duration, std::time::Duration::from_secs(15));
        assert_eq!(cluster.renew_deadline, std::time::Duration::from_secs(10));
        assert_eq!(cluster.retry_period, std::time::Duration::from_secs(2));
        assert_eq!(cluster.poll_interval, std::time::Duration::from_secs(2));
        assert_eq!(cluster.prepare_timeout, std::time::Duration::from_secs(60));
        assert_eq!(cluster.probe, None);
        let config = Config::parse(&format!(
            "{SPEC_EXAMPLE}\n[cluster]\nleadership = \"kubernetes\"\npod = \"web-1\"\nprobe = \"0.0.0.0:9090\"\nprepare_timeout = \"5s\"\n"
        ))
        .unwrap();
        let cluster = config.cluster.unwrap();
        assert_eq!(cluster.pod, "web-1");
        assert_eq!(cluster.probe, Some("0.0.0.0:9090".parse().unwrap()));
        assert_eq!(cluster.prepare_timeout, std::time::Duration::from_secs(5));
    }

    #[test]
    fn cluster_refused_in_immutable_mode() {
        rejects(
            &format!("{IMMUTABLE}\n[cluster]\nleadership = \"kubernetes\"\n"),
            "mode = \"managed\"",
        );
    }

    #[test]
    fn renew_deadline_must_be_below_lease_duration() {
        rejects(
            &format!(
                "{SPEC_EXAMPLE}\n[cluster]\nleadership = \"kubernetes\"\nrenew_deadline = \"15s\"\n"
            ),
            "cluster.renew_deadline",
        );
        rejects(
            &format!(
                "{SPEC_EXAMPLE}\n[cluster]\nleadership = \"kubernetes\"\nretry_period = \"10s\"\n"
            ),
            "cluster.retry_period",
        );
    }

    #[cfg(not(feature = "test-hooks"))]
    #[test]
    fn file_leadership_needs_test_hooks() {
        rejects(
            &format!("{SPEC_EXAMPLE}\n[cluster]\nleadership = \"file\"\n"),
            "test-hooks",
        );
    }

    #[cfg(feature = "test-hooks")]
    #[test]
    fn file_leadership_allowed_with_test_hooks() {
        let config = Config::parse(&format!(
            "{SPEC_EXAMPLE}\n[cluster]\nleadership = \"file\"\n"
        ))
        .unwrap();
        assert_eq!(config.cluster.unwrap().leadership, Leadership::File);
    }

    const MANAGED_BLOG: &str =
        "\n[app.blog]\nkind = \"php\"\nsource = \"https://releases.example.com/blog/index.json\"\n";

    #[test]
    fn php_app_gets_the_pool_defaults() {
        let config = Config::parse(IMMUTABLE).unwrap();
        let php = config.apps[&id("blog")]
            .php
            .clone()
            .expect("a php app has pool settings");
        assert_eq!((php.workers.min, php.workers.max), (1, 4));
        assert_eq!(php.max_requests, 1000);
        assert_eq!(php.request_timeout, std::time::Duration::from_secs(60));
        assert_eq!(php.memory_limit, 256 * 1024 * 1024);
        assert_eq!(php.idle_timeout, std::time::Duration::from_secs(10));
        assert!(php.site_dir.is_none() && php.bindings.is_empty());
        assert!(config.apps[&id("admin")].php.is_none());
    }

    #[test]
    fn php_settings_parse() {
        let config = Config::parse(&format!(
            "{SPEC_EXAMPLE}{RUNTIME}{MANAGED_BLOG}[app.blog.php]\nworkers = {{ min = 2, max = 8 }}\nmax_requests = 50\nrequest_timeout = \"5s\"\nmemory_limit = \"64MiB\"\nidle_timeout = \"1m\"\nsite_dir = \"/srv/blog-site\"\nbindings = {{ \"SpeedySpec\\\\WpHooks\\\\HookStore\" = \"App\\\\Store\" }}\n"
        ))
        .unwrap();
        let php = config.apps[&id("blog")].php.clone().unwrap();
        assert_eq!((php.workers.min, php.workers.max), (2, 8));
        assert_eq!(php.max_requests, 50);
        assert_eq!(php.request_timeout, std::time::Duration::from_secs(5));
        assert_eq!(php.memory_limit, 64 * 1024 * 1024);
        assert_eq!(php.idle_timeout, std::time::Duration::from_secs(60));
        assert_eq!(
            php.site_dir.as_deref(),
            Some(std::path::Path::new("/srv/blog-site"))
        );
        assert_eq!(
            php.bindings,
            vec![(
                "SpeedySpec\\WpHooks\\HookStore".to_string(),
                "App\\Store".to_string()
            )]
        );
        let bytes = Config::parse(&format!("{SPEC_EXAMPLE}{RUNTIME}{MANAGED_BLOG}[app.blog.php]\nsite_dir = \"/s\"\nmemory_limit = 1048576\n")).unwrap();
        assert_eq!(
            bytes.apps[&id("blog")].php.clone().unwrap().memory_limit,
            1_048_576
        );
    }

    #[test]
    fn php_settings_are_checked() {
        rejects(
            &format!("{SPEC_EXAMPLE}{RUNTIME}{MANAGED_BLOG}"),
            "app.blog.php.site_dir",
        );
        rejects(
            &format!(
                "{SPEC_EXAMPLE}{RUNTIME}{MANAGED_BLOG}[app.blog.php]\nsite_dir = \"/s\"\nworkers = {{ min = 3, max = 2 }}\n"
            ),
            "app.blog.php.workers",
        );
        rejects(
            &format!(
                "{SPEC_EXAMPLE}{RUNTIME}{MANAGED_BLOG}[app.blog.php]\nsite_dir = \"/s\"\nmemory_limit = \"lots\"\n"
            ),
            "app.blog.php.memory_limit",
        );
        rejects(
            &IMMUTABLE.replace(
                "kind = \"static\"\npath = \"/srv/admin\"",
                "kind = \"static\"\npath = \"/srv/admin\"\n[app.admin.php]\nmax_requests = 5",
            ),
            "app.admin.php",
        );
    }

    const RUNTIME: &str =
        "\n[app.rt]\nkind = \"runtime\"\nsource = \"https://releases.example.com/rt/index.json\"\n";
    const BLOG_SITE: &str = "[app.blog.php]\nsite_dir = \"/s\"\n";

    #[test]
    fn php_app_defaults_to_the_only_runtime() {
        let config =
            Config::parse(&format!("{SPEC_EXAMPLE}{RUNTIME}{MANAGED_BLOG}{BLOG_SITE}")).unwrap();
        assert_eq!(config.apps[&id("blog")].runtime, Some(id("rt")));
        assert_eq!(config.apps[&id("rt")].kind, AppKind::Runtime);
        assert_eq!(config.apps[&id("rt")].runtime, None);
    }

    #[test]
    fn php_app_with_two_runtimes_must_name_one() {
        let second = "\n[app.rt2]\nkind = \"runtime\"\nsource = \"https://releases.example.com/rt2/index.json\"\n";
        rejects(
            &format!("{SPEC_EXAMPLE}{RUNTIME}{second}{MANAGED_BLOG}{BLOG_SITE}"),
            "app.blog.runtime",
        );
        let named = MANAGED_BLOG.replace("kind = \"php\"", "kind = \"php\"\nruntime = \"rt2\"");
        let config = Config::parse(&format!(
            "{SPEC_EXAMPLE}{RUNTIME}{second}{named}{BLOG_SITE}"
        ))
        .unwrap();
        assert_eq!(config.apps[&id("blog")].runtime, Some(id("rt2")));
    }

    #[test]
    fn runtime_must_name_a_runtime_app() {
        let named = MANAGED_BLOG.replace("kind = \"php\"", "kind = \"php\"\nruntime = \"admin\"");
        rejects(
            &format!("{SPEC_EXAMPLE}{RUNTIME}{named}{BLOG_SITE}"),
            "app.blog.runtime",
        );
        rejects(
            &format!(
                "{SPEC_EXAMPLE}{}",
                RUNTIME.replace("kind = \"runtime\"", "kind = \"runtime\"\nruntime = \"rt\"")
            ),
            "app.rt.runtime",
        );
    }

    #[test]
    fn immutable_runtime_from_path() {
        let config = Config::parse(IMMUTABLE).unwrap();
        assert_eq!(
            config.apps[&id("rt")].path.as_deref(),
            Some(std::path::Path::new("/opt/rt/1.0.0"))
        );
        assert_eq!(config.apps[&id("blog")].runtime, Some(id("rt")));
    }

    #[test]
    fn php_app_with_no_runtime_is_an_error() {
        rejects(
            &format!("{SPEC_EXAMPLE}{MANAGED_BLOG}{BLOG_SITE}"),
            "app.blog.runtime",
        );
    }

    #[test]
    fn env_sources_merge_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let site = dir.path().join("site");
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(site.join(".env"), "# comment\nA=1\nB=1\n").unwrap();
        let file = dir.path().join("blog.env");
        std::fs::write(&file, "B=2\nC=2\n").unwrap();
        let config = Config::parse(&format!(
            "{SPEC_EXAMPLE}{RUNTIME}{MANAGED_BLOG}[app.blog.php]\nsite_dir = \"{}\"\nenv_file = \"{}\"\nenv = {{ C = \"3\" }}\n",
            site.display(),
            file.display()
        ))
        .unwrap();
        let php = config.apps[&id("blog")].php.clone().unwrap();
        assert_eq!(
            php.worker_env().unwrap(),
            vec![
                ("A".to_string(), "1".to_string()),
                ("B".to_string(), "2".to_string()),
                ("C".to_string(), "3".to_string())
            ]
        );
        std::fs::remove_file(&file).unwrap();
        let err = php.worker_env().unwrap_err();
        assert!(err.contains("blog.env"), "{err}");
    }

    #[test]
    fn the_server_app_has_no_mounts() {
        rejects(
            &format!("{IMMUTABLE}\n[[mount]]\npath = \"/srv/\"\napp = \"server\"\n"),
            "the server app, which has no mounts",
        );
        let config = Config::parse(&format!(
            "{IMMUTABLE}\n[[mount]]\npath = \"/admin/api/\"\napp = \"blog\"\naccess = \"signed-in\"\n"
        ))
        .unwrap();
        let api = config.mounts.resolve("other.org", "/admin/api/x").unwrap();
        assert_eq!(
            api.app.as_str(),
            "blog",
            "/admin/api/ is an ordinary path now"
        );
    }

    #[test]
    fn gate_settings_parse() {
        let config = Config::parse(&format!(
            "{IMMUTABLE}\n[access]\nimplementation = \"gate\"\n\n[gate]\napp = \"blog\"\nsession_ttl = \"30s\"\nmax_pending = 4\nmaintenance_apps = [\"blog\"]\n"
        ))
        .unwrap();
        assert_eq!(config.access, AccessImpl::Gate);
        let gate = config.gate.unwrap();
        assert_eq!(gate.app, id("blog"));
        assert_eq!(gate.session_ttl, Duration::from_secs(30));
        assert_eq!(gate.timeout, Duration::from_secs(5));
        assert_eq!(gate.site_ttl, Duration::from_secs(2));
        assert_eq!(gate.poll, Duration::from_secs(1));
        assert_eq!(gate.max_pending, 4);
        assert_eq!(gate.maintenance_apps, Some(vec![id("blog")]));
    }

    #[test]
    fn gate_needs_a_php_gate_app() {
        rejects(
            &format!("{IMMUTABLE}\n[access]\nimplementation = \"gate\"\n"),
            "gate.app",
        );
        rejects(
            &format!(
                "{IMMUTABLE}\n[access]\nimplementation = \"gate\"\n\n[gate]\napp = \"admin\"\n"
            ),
            "gate.app",
        );
    }
}
