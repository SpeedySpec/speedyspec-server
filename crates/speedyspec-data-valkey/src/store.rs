//! `ValkeyStore`: keys stored as `<prefix><keyspace>:<key>`, one connection made on first use and remade once after
//! a loss.

use std::cell::RefCell;
use std::ops::Bound;
use std::time::Duration;

use redis::{Client, Connection, RedisError, Value};
use speedyspec_data::{Capabilities, DbError, KeyValues, NoSqlStore, UpsertForm};

pub struct ValkeyStore {
    engine: String,
    client: Client,
    host: String,
    prefix: String,
    timeout: Duration,
    conn: RefCell<Option<Connection>>,
}

/// The driver's URL for a `valkey://` one: `redis://`, or `rediss://` over TLS.
pub fn driver_url(url: &str, tls: bool) -> Result<String, DbError> {
    let rest = url.strip_prefix("valkey://").ok_or_else(|| {
        DbError::Config(format!(
            "a Valkey URL starts with valkey://, not `{}`",
            redact(url)
        ))
    })?;
    Ok(format!("{}://{rest}", if tls { "rediss" } else { "redis" }))
}

/// The URL without its password.
fn redact(url: &str) -> String {
    match (url.find("://"), url.rfind('@')) {
        (Some(s), Some(at)) if at > s => format!("{}***{}", &url[..s + 3], &url[at..]),
        _ => url.to_string(),
    }
}

fn valid_keyspace(keyspace: &str) -> bool {
    !keyspace.is_empty()
        && keyspace
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

impl ValkeyStore {
    pub fn new(
        engine: &str,
        url: &str,
        prefix: &str,
        timeout: Duration,
    ) -> Result<ValkeyStore, DbError> {
        let valid = !prefix.is_empty()
            && prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'));
        if !valid {
            return Err(DbError::Config(format!(
                "engine `{engine}`: the Valkey prefix `{prefix}` must match [A-Za-z0-9_.:-]+"
            )));
        }
        let client =
            Client::open(url).map_err(|e| DbError::Config(format!("engine `{engine}`: {e}")))?;
        let info = client.get_connection_info();
        let host = format!("{}", info.addr);
        Ok(ValkeyStore {
            engine: engine.to_string(),
            client,
            host,
            prefix: prefix.to_string(),
            timeout,
            conn: RefCell::new(None),
        })
    }

    fn key(&self, keyspace: &str, key: &[u8]) -> Result<Vec<u8>, DbError> {
        if !valid_keyspace(keyspace) {
            return Err(DbError::Config(format!(
                "the keyspace `{keyspace}` must match [a-z0-9_-]+"
            )));
        }
        let mut out = format!("{}{keyspace}:", self.prefix).into_bytes();
        out.extend_from_slice(key);
        Ok(out)
    }

    fn lost(e: &RedisError) -> bool {
        e.is_io_error() || e.is_connection_dropped() || e.is_connection_refusal() || e.is_timeout()
    }

    fn map(&self, e: RedisError) -> DbError {
        if Self::lost(&e) {
            return DbError::Connection {
                engine: self.engine.clone(),
                message: format!("valkey at {}: {}", self.host, format!("{:?}", e.kind())),
            };
        }
        let detail = e.detail().unwrap_or_default().to_string();
        let code = if detail.contains("not an integer") {
            "not-integer".to_string()
        } else if detail.contains("overflow") {
            "overflow".to_string()
        } else {
            e.code().unwrap_or("valkey").to_string()
        };
        DbError::Engine {
            engine: self.engine.clone(),
            code: Some(code),
            message: e.to_string(),
        }
    }

    fn connect(&self) -> Result<Connection, DbError> {
        let conn = self
            .client
            .get_connection_with_timeout(self.timeout)
            .map_err(|e| DbError::Connection {
                engine: self.engine.clone(),
                message: format!("valkey at {}: {}", self.host, format!("{:?}", e.kind())),
            })?;
        conn.set_read_timeout(Some(self.timeout))
            .map_err(|e| self.map(e))?;
        conn.set_write_timeout(Some(self.timeout))
            .map_err(|e| self.map(e))?;
        Ok(conn)
    }

    /// Runs `f` on the connection; on a lost connection, `retry` remakes it and runs `f` once more.
    fn with<T>(
        &self,
        retry: bool,
        f: impl Fn(&mut Connection) -> Result<T, RedisError>,
    ) -> Result<T, DbError> {
        let attempts = if retry { 2 } else { 1 };
        for attempt in 0..attempts {
            if self.conn.borrow().is_none() {
                *self.conn.borrow_mut() = Some(self.connect()?);
            }
            let result = {
                let mut guard = self.conn.borrow_mut();
                f(guard.as_mut().expect("connected above"))
            };
            match result {
                Ok(v) => return Ok(v),
                Err(e) if Self::lost(&e) => {
                    *self.conn.borrow_mut() = None;
                    if attempt + 1 == attempts {
                        return Err(self.map(e));
                    }
                }
                Err(e) => return Err(self.map(e)),
            }
        }
        unreachable!("the loop returns")
    }
}

/// A TTL as Valkey's PX: at least 1 ms, rounded up.
fn millis(ttl: Duration) -> u64 {
    (ttl.as_micros().div_ceil(1000) as u64).max(1)
}

impl NoSqlStore for ValkeyStore {
    fn engine(&self) -> &str {
        &self.engine
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            transactions: false,
            savepoints: false,
            ordered_ranges: false,
            expiry: true,
            atomic_increment: true,
            returning: false,
            upsert: UpsertForm::None,
        }
    }

    fn get(&self, keyspace: &str, key: &[u8]) -> Result<Option<Vec<u8>>, DbError> {
        let k = self.key(keyspace, key)?;
        self.with(true, |c| redis::cmd("GET").arg(&k).query(c))
    }

    fn get_many(&self, keyspace: &str, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>, DbError> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let ks = keys
            .iter()
            .map(|k| self.key(keyspace, k))
            .collect::<Result<Vec<_>, _>>()?;
        self.with(true, |c| {
            let mut cmd = redis::cmd("MGET");
            for k in &ks {
                cmd.arg(k);
            }
            cmd.query(c)
        })
    }

    fn put(
        &self,
        keyspace: &str,
        key: &[u8],
        value: &[u8],
        ttl: Option<Duration>,
    ) -> Result<(), DbError> {
        let k = self.key(keyspace, key)?;
        match ttl {
            Some(t) if t.is_zero() => self
                .with(true, |c| redis::cmd("DEL").arg(&k).query::<i64>(c))
                .map(|_| ()),
            Some(t) => self.with(true, |c| {
                redis::cmd("SET")
                    .arg(&k)
                    .arg(value)
                    .arg("PX")
                    .arg(millis(t))
                    .query(c)
            }),
            None => self.with(true, |c| redis::cmd("SET").arg(&k).arg(value).query(c)),
        }
    }

    fn put_if_absent(
        &self,
        keyspace: &str,
        key: &[u8],
        value: &[u8],
        ttl: Option<Duration>,
    ) -> Result<bool, DbError> {
        let k = self.key(keyspace, key)?;
        if matches!(ttl, Some(t) if t.is_zero()) {
            return Ok(false);
        }
        // Not retried: a lost reply cannot tell whether the key was written.
        let reply: Value = self.with(false, |c| {
            let mut cmd = redis::cmd("SET");
            cmd.arg(&k).arg(value).arg("NX");
            if let Some(t) = ttl {
                cmd.arg("PX").arg(millis(t));
            }
            cmd.query(c)
        })?;
        Ok(!matches!(reply, Value::Nil))
    }

    fn delete(&self, keyspace: &str, key: &[u8]) -> Result<bool, DbError> {
        let k = self.key(keyspace, key)?;
        self.with(true, |c| redis::cmd("DEL").arg(&k).query::<i64>(c))
            .map(|n| n == 1)
    }

    fn range(
        &self,
        keyspace: &str,
        _start: Bound<&[u8]>,
        _end: Bound<&[u8]>,
        _limit: Option<usize>,
    ) -> Result<KeyValues, DbError> {
        self.key(keyspace, b"")?;
        Err(DbError::Engine {
            engine: self.engine.clone(),
            code: Some("unsupported".into()),
            message: "ordered ranges are not supported by valkey".into(),
        })
    }

    fn increment(&self, keyspace: &str, key: &[u8], by: i64) -> Result<i64, DbError> {
        let k = self.key(keyspace, key)?;
        // Never retried: an increment is not idempotent.
        self.with(false, |c| redis::cmd("INCRBY").arg(&k).arg(by).query(c))
    }

    fn clear(&self, keyspace: &str) -> Result<(), DbError> {
        let mut pattern = self.key(keyspace, b"")?;
        pattern.push(b'*');
        let mut cursor: u64 = 0;
        loop {
            let (next, keys): (u64, Vec<Vec<u8>>) = self.with(true, |c| {
                redis::cmd("SCAN")
                    .arg(cursor)
                    .arg("MATCH")
                    .arg(&pattern)
                    .arg("COUNT")
                    .arg(1000)
                    .query(c)
            })?;
            if !keys.is_empty() {
                self.with(true, |c| {
                    let mut cmd = redis::cmd("UNLINK");
                    for k in &keys {
                        cmd.arg(k);
                    }
                    cmd.query::<i64>(c)
                })?;
            }
            if next == 0 {
                return Ok(());
            }
            cursor = next;
        }
    }
}
