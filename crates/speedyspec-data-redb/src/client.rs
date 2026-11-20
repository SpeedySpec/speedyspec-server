//! `RedbClient`: a Remote Proxy for the owner's `RedbStore`, one connection per worker over the owner's socket.

use std::cell::RefCell;
use std::ops::Bound;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use speedyspec_data::{Capabilities, DbError, KeyValues, NoSqlStore};

use crate::protocol::{PROTOCOL_VERSION, Request, Response, WireBound, read_frame, write_frame};
use crate::socket::check_socket_path;

pub struct RedbClient {
    engine: String,
    socket: PathBuf,
    timeout: Duration,
    conn: RefCell<Option<UnixStream>>,
}

/// A TTL on the wire: at least 1 ms, rounded up; a zero TTL stays 0, which deletes.
fn ttl_ms(ttl: Option<Duration>) -> Option<u64> {
    ttl.map(|t| {
        if t.is_zero() {
            0
        } else {
            u64::try_from(t.as_nanos().div_ceil(1_000_000))
                .unwrap_or(u64::MAX)
                .max(1)
        }
    })
}

fn wire(b: Bound<&[u8]>) -> WireBound {
    match b {
        Bound::Included(k) => WireBound::Included(k.to_vec()),
        Bound::Excluded(k) => WireBound::Excluded(k.to_vec()),
        Bound::Unbounded => WireBound::Unbounded,
    }
}

impl RedbClient {
    /// Checks the socket path now; connects on first use.
    pub fn new(engine: &str, socket: PathBuf, timeout: Duration) -> Result<RedbClient, DbError> {
        check_socket_path(&socket)?;
        Ok(RedbClient {
            engine: engine.to_string(),
            socket,
            timeout,
            conn: RefCell::new(None),
        })
    }

    fn engine_error(&self, code: &str, message: String) -> DbError {
        DbError::Engine {
            engine: self.engine.clone(),
            code: Some(code.to_string()),
            message,
        }
    }

    /// Connects and says `Hello`.
    fn connect(&self) -> Result<UnixStream, DbError> {
        let unreachable = |e: std::io::Error| DbError::Connection {
            engine: self.engine.clone(),
            message: format!(
                "the redb owner is not reachable at {}: {e}",
                self.socket.display()
            ),
        };
        let mut stream = UnixStream::connect(&self.socket).map_err(unreachable)?;
        let timeout = Some(self.timeout).filter(|t| !t.is_zero());
        stream.set_read_timeout(timeout).map_err(unreachable)?;
        stream.set_write_timeout(timeout).map_err(unreachable)?;
        write_frame(
            &mut stream,
            &Request::Hello {
                version: PROTOCOL_VERSION,
            },
        )
        .map_err(|e| self.rename(e))?;
        match read_frame::<Response>(&mut stream).map_err(|e| self.rename(e))? {
            Response::Ready { .. } => Ok(stream),
            Response::Error { code, message } => Err(self.engine_error(&code, message)),
            other => Err(self.engine_error(
                "protocol",
                format!("the owner answered Hello with {other:?}"),
            )),
        }
    }

    /// The frame layer's errors carry this client's engine name.
    fn rename(&self, e: DbError) -> DbError {
        match e {
            DbError::Connection { message, .. } => DbError::Connection {
                engine: self.engine.clone(),
                message,
            },
            DbError::Engine { code, message, .. } => DbError::Engine {
                engine: self.engine.clone(),
                code,
                message,
            },
            other => other,
        }
    }

    /// One request, one response; a lost connection is dropped, then remade and retried once when `retry`.
    fn call(&self, retry: bool, request: &Request) -> Result<Response, DbError> {
        let attempts = if retry { 2 } else { 1 };
        let mut last = None;
        for _ in 0..attempts {
            let result = self.exchange(request);
            match result {
                Err(e @ DbError::Connection { .. }) => {
                    *self.conn.borrow_mut() = None;
                    last = Some(e);
                }
                Ok(Response::Error { code, message }) => {
                    return Err(self.engine_error(&code, message));
                }
                other => return other,
            }
        }
        Err(last.expect("at least one attempt"))
    }

    fn exchange(&self, request: &Request) -> Result<Response, DbError> {
        if self.conn.borrow().is_none() {
            let stream = self.connect()?;
            *self.conn.borrow_mut() = Some(stream);
        }
        let mut guard = self.conn.borrow_mut();
        let stream = guard.as_mut().expect("connected above");
        write_frame(stream, request).map_err(|e| self.rename(e))?;
        read_frame(stream).map_err(|e| self.rename(e))
    }

    /// The answer was not the variant the request expects: the connection is out of step, so drop it.
    fn unexpected(&self, response: Response) -> DbError {
        *self.conn.borrow_mut() = None;
        self.engine_error("protocol", format!("unexpected response {response:?}"))
    }
}

impl NoSqlStore for RedbClient {
    fn engine(&self) -> &str {
        &self.engine
    }

    fn capabilities(&self) -> Capabilities {
        crate::store::capabilities()
    }

    fn get(&self, keyspace: &str, key: &[u8]) -> Result<Option<Vec<u8>>, DbError> {
        match self.call(
            true,
            &Request::Get {
                keyspace: keyspace.into(),
                key: key.to_vec(),
            },
        )? {
            Response::Value(v) => Ok(v),
            other => Err(self.unexpected(other)),
        }
    }

    fn get_many(&self, keyspace: &str, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>, DbError> {
        let keys = keys.iter().map(|k| k.to_vec()).collect();
        match self.call(
            true,
            &Request::GetMany {
                keyspace: keyspace.into(),
                keys,
            },
        )? {
            Response::Values(v) => Ok(v),
            other => Err(self.unexpected(other)),
        }
    }

    fn put(
        &self,
        keyspace: &str,
        key: &[u8],
        value: &[u8],
        ttl: Option<Duration>,
    ) -> Result<(), DbError> {
        let request = Request::Put {
            keyspace: keyspace.into(),
            key: key.to_vec(),
            value: value.to_vec(),
            ttl_ms: ttl_ms(ttl),
        };
        match self.call(true, &request)? {
            Response::Done => Ok(()),
            other => Err(self.unexpected(other)),
        }
    }

    /// Not retried: a lost reply cannot say whether the key was written.
    fn put_if_absent(
        &self,
        keyspace: &str,
        key: &[u8],
        value: &[u8],
        ttl: Option<Duration>,
    ) -> Result<bool, DbError> {
        let request = Request::PutIfAbsent {
            keyspace: keyspace.into(),
            key: key.to_vec(),
            value: value.to_vec(),
            ttl_ms: ttl_ms(ttl),
        };
        match self.call(false, &request)? {
            Response::Written(w) => Ok(w),
            other => Err(self.unexpected(other)),
        }
    }

    fn delete(&self, keyspace: &str, key: &[u8]) -> Result<bool, DbError> {
        match self.call(
            true,
            &Request::Delete {
                keyspace: keyspace.into(),
                key: key.to_vec(),
            },
        )? {
            Response::Deleted(d) => Ok(d),
            other => Err(self.unexpected(other)),
        }
    }

    fn range(
        &self,
        keyspace: &str,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
        limit: Option<usize>,
    ) -> Result<KeyValues, DbError> {
        let request = Request::Range {
            keyspace: keyspace.into(),
            start: wire(start),
            end: wire(end),
            limit: limit.map(|n| n as u64),
        };
        match self.call(true, &request)? {
            Response::Entries(e) => Ok(e),
            other => Err(self.unexpected(other)),
        }
    }

    /// Never retried: it is not idempotent.
    fn increment(&self, keyspace: &str, key: &[u8], by: i64) -> Result<i64, DbError> {
        match self.call(
            false,
            &Request::Increment {
                keyspace: keyspace.into(),
                key: key.to_vec(),
                by,
            },
        )? {
            Response::Counter(n) => Ok(n),
            other => Err(self.unexpected(other)),
        }
    }

    fn clear(&self, keyspace: &str) -> Result<(), DbError> {
        match self.call(
            true,
            &Request::Clear {
                keyspace: keyspace.into(),
            },
        )? {
            Response::Done => Ok(()),
            other => Err(self.unexpected(other)),
        }
    }
}
