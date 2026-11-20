//! `RedbOwner`: the one process that opens a site's ReDB file and serves it to every worker over a private socket.

use std::fs;
use std::net::Shutdown;
use std::ops::Bound;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use speedyspec_data::{DbError, EngineConfig, NoSqlStore};

use crate::protocol::{PROTOCOL_VERSION, Request, Response, WireBound, read_frame, write_frame};
use crate::socket::{check_socket_path, owner_socket_for};
use crate::store::RedbStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerConfig {
    pub engine: String,
    pub file: PathBuf,
    pub socket: PathBuf,
    pub sweep_every: Duration,
}

impl OwnerConfig {
    /// The owner's configuration from the engine's: a `redb:///absolute/file.redb` URL, options `socket` (default
    /// `owner_socket_for(file)`) and `sweep_secs` (default 60).
    pub fn from_engine_config(config: &EngineConfig) -> Result<OwnerConfig, DbError> {
        let file = config
            .url
            .strip_prefix("redb://")
            .filter(|p| p.starts_with('/'))
            .map(PathBuf::from)
            .ok_or_else(|| {
                DbError::Config(format!(
                    "engine `{}`: a ReDB URL is redb:///absolute/file.redb, not `{}`",
                    config.name, config.url
                ))
            })?;
        let socket = config
            .options
            .get("socket")
            .map(PathBuf::from)
            .unwrap_or_else(|| owner_socket_for(&file));
        let sweep_every = match config.options.get("sweep_secs") {
            None => Duration::from_secs(60),
            Some(s) => s
                .parse::<u64>()
                .ok()
                .filter(|&n| n > 0)
                .map(Duration::from_secs)
                .ok_or_else(|| {
                    DbError::Config(format!(
                        "engine `{}`: sweep_secs must be a positive whole number, not `{s}`",
                        config.name
                    ))
                })?,
        };
        Ok(OwnerConfig {
            engine: config.name.clone(),
            file,
            socket,
            sweep_every,
        })
    }
}

#[derive(Debug)]
pub enum OwnerError {
    /// Another owner already answers on the socket.
    AlreadyRunning,
    Boot(DbError),
}

/// Stops a serving owner from any thread.
#[derive(Debug, Clone)]
pub struct StopHandle {
    stopped: Arc<AtomicBool>,
    socket: PathBuf,
}

impl StopHandle {
    /// Sets the flag, then connects once to wake `accept`.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = UnixStream::connect(&self.socket);
    }
}

pub struct RedbOwner {
    config: OwnerConfig,
    store: Arc<RedbStore>,
    listener: UnixListener,
    stopped: Arc<AtomicBool>,
}

fn boot(e: std::io::Error, what: &str, path: &Path) -> OwnerError {
    OwnerError::Boot(DbError::Config(format!(
        "cannot {what} {}: {e}",
        path.display()
    )))
}

impl RedbOwner {
    /// Prepares the private socket directory, replaces a stale socket, opens the file and binds the socket.
    pub fn open(config: OwnerConfig) -> Result<RedbOwner, OwnerError> {
        check_socket_path(&config.socket).map_err(OwnerError::Boot)?;
        let dir = config.socket.parent().unwrap_or_else(|| Path::new("."));
        match fs::metadata(dir) {
            // A directory the owner did not make is someone else's: refuse one others can enter, never change it.
            Ok(meta) => {
                let mode = meta.permissions().mode() & 0o777;
                if mode & 0o077 != 0 {
                    return Err(OwnerError::Boot(DbError::Config(format!(
                        "the redb owner socket's directory {} is open to others (mode {mode:o}); give the socket \
                         a directory of its own",
                        dir.display()
                    ))));
                }
            }
            Err(_) => {
                fs::create_dir_all(dir).map_err(|e| boot(e, "create", dir))?;
                fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
                    .map_err(|e| boot(e, "restrict", dir))?;
            }
        }
        if fs::symlink_metadata(&config.socket).is_ok() {
            if UnixStream::connect(&config.socket).is_ok() {
                return Err(OwnerError::AlreadyRunning);
            }
            fs::remove_file(&config.socket)
                .map_err(|e| boot(e, "remove the stale socket", &config.socket))?;
        }
        let store = RedbStore::open(&config.engine, &config.file).map_err(OwnerError::Boot)?;
        let listener =
            UnixListener::bind(&config.socket).map_err(|e| boot(e, "bind", &config.socket))?;
        fs::set_permissions(&config.socket, fs::Permissions::from_mode(0o600))
            .map_err(|e| boot(e, "restrict", &config.socket))?;
        Ok(RedbOwner {
            config,
            store: Arc::new(store),
            listener,
            stopped: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn stop_handle(&self) -> StopHandle {
        StopHandle {
            stopped: self.stopped.clone(),
            socket: self.config.socket.clone(),
        }
    }

    /// Serves until stopped: a thread per connection, one sweeping expired entries. On stop every connection is
    /// shut down and joined, so the file is free for the next owner once the owner is dropped.
    pub fn serve(&self) -> Result<(), DbError> {
        // Each live connection's thread, with a handle on its socket so stop can shut it down.
        let mut workers: Vec<(JoinHandle<()>, Option<UnixStream>)> = Vec::new();
        let sweeper = {
            let (store, stopped, every) = (
                self.store.clone(),
                self.stopped.clone(),
                self.config.sweep_every,
            );
            std::thread::spawn(move || sweep(&store, &stopped, every))
        };
        let mut result = Ok(());
        for stream in self.listener.incoming() {
            if self.stopped.load(Ordering::SeqCst) {
                break;
            }
            let stream = match stream {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    result = Err(DbError::Connection {
                        engine: self.config.engine.clone(),
                        message: format!("accept on {} failed: {e}", self.config.socket.display()),
                    });
                    break;
                }
            };
            let copy = stream.try_clone().ok();
            let store = self.store.clone();
            workers.retain(|(w, _)| !w.is_finished());
            workers.push((std::thread::spawn(move || connection(&store, stream)), copy));
        }
        self.stopped.store(true, Ordering::SeqCst);
        for (_, s) in &workers {
            if let Some(s) = s {
                let _ = s.shutdown(Shutdown::Both);
            }
        }
        for (w, _) in workers {
            let _ = w.join();
        }
        let _ = sweeper.join();
        // Writes commit without a sync; a clean stop makes them durable.
        result.and(self.store.sync())
    }
}

/// Sweeps every `every` until stopped, checking the flag often so stop is prompt.
fn sweep(store: &RedbStore, stopped: &AtomicBool, every: Duration) {
    let mut next = Instant::now() + every;
    while !stopped.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(50));
        if Instant::now() >= next {
            let _ = store.sweep_all_expired(1000).and_then(|_| store.sync());
            next = Instant::now() + every;
        }
    }
}

fn protocol_error(message: &str) -> Response {
    Response::Error {
        code: "protocol".into(),
        message: message.into(),
    }
}

/// One worker's connection, shut down when it ends (the owner's handle on it would otherwise keep it open).
fn connection(store: &RedbStore, mut stream: UnixStream) {
    converse(store, &mut stream);
    let _ = stream.shutdown(Shutdown::Both);
}

/// `Hello` first, then one `Response` per `Request`, in order.
fn converse(store: &RedbStore, stream: &mut UnixStream) {
    match read_frame::<Request>(stream) {
        Ok(Request::Hello { version }) if version == PROTOCOL_VERSION => {
            if write_frame(
                stream,
                &Response::Ready {
                    version: PROTOCOL_VERSION,
                },
            )
            .is_err()
            {
                return;
            }
        }
        Ok(Request::Hello { version }) => {
            let message = format!("protocol version {version} is not {PROTOCOL_VERSION}");
            let _ = write_frame(stream, &protocol_error(&message));
            return;
        }
        Ok(_) => {
            let _ = write_frame(stream, &protocol_error("the first frame must be Hello"));
            return;
        }
        Err(DbError::Connection { .. }) => return,
        Err(e) => {
            let _ = write_frame(stream, &protocol_error(&e.to_string()));
            return;
        }
    }
    loop {
        let request = match read_frame::<Request>(stream) {
            Ok(r) => r,
            Err(DbError::Connection { .. }) => return,
            Err(e) => {
                let _ = write_frame(stream, &protocol_error(&e.to_string()));
                return;
            }
        };
        if write_frame(stream, &answer(store, request)).is_err() {
            return;
        }
    }
}

fn bound(b: &WireBound) -> Bound<&[u8]> {
    match b {
        WireBound::Included(k) => Bound::Included(k),
        WireBound::Excluded(k) => Bound::Excluded(k),
        WireBound::Unbounded => Bound::Unbounded,
    }
}

/// The store's answer to one request; `ttl_ms: Some(0)` deletes.
fn answer(store: &RedbStore, request: Request) -> Response {
    let ttl = |ms: Option<u64>| ms.map(Duration::from_millis);
    let result = match request {
        Request::Hello { .. } => Ok(Response::Ready {
            version: PROTOCOL_VERSION,
        }),
        Request::Get { keyspace, key } => store.get(&keyspace, &key).map(Response::Value),
        Request::GetMany { keyspace, keys } => {
            let keys: Vec<&[u8]> = keys.iter().map(Vec::as_slice).collect();
            store.get_many(&keyspace, &keys).map(Response::Values)
        }
        Request::Put {
            keyspace,
            key,
            value,
            ttl_ms,
        } => store
            .put(&keyspace, &key, &value, ttl(ttl_ms))
            .map(|()| Response::Done),
        Request::PutIfAbsent {
            keyspace,
            key,
            value,
            ttl_ms,
        } => store
            .put_if_absent(&keyspace, &key, &value, ttl(ttl_ms))
            .map(Response::Written),
        Request::Delete { keyspace, key } => store.delete(&keyspace, &key).map(Response::Deleted),
        Request::Range {
            keyspace,
            start,
            end,
            limit,
        } => store
            .range(
                &keyspace,
                bound(&start),
                bound(&end),
                limit.map(|n| usize::try_from(n).unwrap_or(usize::MAX)),
            )
            .map(Response::Entries),
        Request::Increment { keyspace, key, by } => {
            store.increment(&keyspace, &key, by).map(Response::Counter)
        }
        Request::Clear { keyspace } => store.clear(&keyspace).map(|()| Response::Done),
    };
    result.unwrap_or_else(|e| match e {
        DbError::Engine {
            code: Some(code),
            message,
            ..
        } => Response::Error { code, message },
        other => Response::Error {
            code: "storage".into(),
            message: other.to_string(),
        },
    })
}

fn log(event: &str, config: &OwnerConfig, message: &str) {
    let line = serde_json::json!({
        "event": event,
        "engine": config.engine,
        "socket": config.socket,
        "message": message,
    });
    eprintln!("{line}");
}

/// The entry point the runtime calls in its own supervised process: `0` when stopped, `1` on a boot failure, `3`
/// when another owner already answers on the socket.
pub fn run_owner(config: OwnerConfig) -> i32 {
    let owner = match RedbOwner::open(config.clone()) {
        Ok(o) => o,
        Err(OwnerError::AlreadyRunning) => {
            log(
                "redb_owner_already_running",
                &config,
                "another owner answers on the socket",
            );
            return 3;
        }
        Err(OwnerError::Boot(e)) => {
            log("redb_owner_boot_failed", &config, &e.to_string());
            return 1;
        }
    };
    log(
        "redb_owner_started",
        &config,
        &format!("serving {}", config.file.display()),
    );
    match owner.serve() {
        Ok(()) => {
            log("redb_owner_stopped", &config, "stopped");
            0
        }
        Err(e) => {
            log("redb_owner_boot_failed", &config, &e.to_string());
            1
        }
    }
}
