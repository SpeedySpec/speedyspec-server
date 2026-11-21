//! The owner's wire protocol: each frame is a 4-byte big-endian length, then that many `postcard` bytes.

use std::io::{ErrorKind, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use speedyspec_data::DbError;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME: u32 = 64 * 1024 * 1024;

/// The engine name protocol errors carry: the frame layer knows no configured engine.
const ENGINE: &str = "redb";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireBound {
    Included(Vec<u8>),
    Excluded(Vec<u8>),
    Unbounded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    Hello {
        version: u16,
    },
    Get {
        keyspace: String,
        key: Vec<u8>,
    },
    GetMany {
        keyspace: String,
        keys: Vec<Vec<u8>>,
    },
    Put {
        keyspace: String,
        key: Vec<u8>,
        value: Vec<u8>,
        ttl_ms: Option<u64>,
    },
    PutIfAbsent {
        keyspace: String,
        key: Vec<u8>,
        value: Vec<u8>,
        ttl_ms: Option<u64>,
    },
    Delete {
        keyspace: String,
        key: Vec<u8>,
    },
    Range {
        keyspace: String,
        start: WireBound,
        end: WireBound,
        limit: Option<u64>,
    },
    Increment {
        keyspace: String,
        key: Vec<u8>,
        by: i64,
    },
    Clear {
        keyspace: String,
    },
    // Appended, so the earlier variants keep their wire numbers.
    AdjustExisting {
        keyspace: String,
        key: Vec<u8>,
        by: i64,
        floor: Option<i64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    Ready { version: u16 },
    Value(Option<Vec<u8>>),
    Values(Vec<Option<Vec<u8>>>),
    Done,
    Written(bool),
    Deleted(bool),
    Entries(Vec<(Vec<u8>, Vec<u8>)>),
    Counter(i64),
    Error { code: String, message: String },
    // Appended, so the earlier variants keep their wire numbers.
    MaybeCounter(Option<i64>),
}

fn connection(e: std::io::Error) -> DbError {
    let message = if e.kind() == ErrorKind::UnexpectedEof {
        "the redb owner connection closed".to_string()
    } else {
        format!("the redb owner connection failed: {e}")
    };
    DbError::Connection {
        engine: ENGINE.to_string(),
        message,
    }
}

fn engine(code: &str, message: String) -> DbError {
    DbError::Engine {
        engine: ENGINE.to_string(),
        code: Some(code.to_string()),
        message,
    }
}

/// Writes `msg` as one frame.
pub fn write_frame(w: &mut impl Write, msg: &impl Serialize) -> Result<(), DbError> {
    let body = postcard::to_stdvec(msg)
        .map_err(|e| engine("protocol", format!("cannot encode a frame: {e}")))?;
    let len = u32::try_from(body.len())
        .ok()
        .filter(|&n| n <= MAX_FRAME)
        .ok_or_else(|| {
            engine(
                "too-large",
                format!("a frame of {} bytes exceeds {MAX_FRAME}", body.len()),
            )
        })?;
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend(len.to_be_bytes());
    frame.extend(body);
    w.write_all(&frame)
        .and_then(|()| w.flush())
        .map_err(connection)
}

/// Reads one frame: `too-large` past `MAX_FRAME`, `protocol` when it does not decode, `Connection` on EOF or IO failure.
pub fn read_frame<T: DeserializeOwned>(r: &mut impl Read) -> Result<T, DbError> {
    let mut head = [0; 4];
    r.read_exact(&mut head).map_err(connection)?;
    let len = u32::from_be_bytes(head);
    if len > MAX_FRAME {
        return Err(engine(
            "too-large",
            format!("a frame of {len} bytes exceeds {MAX_FRAME}"),
        ));
    }
    let mut body = vec![0; len as usize];
    r.read_exact(&mut body).map_err(connection)?;
    postcard::from_bytes(&body)
        .map_err(|e| engine("protocol", format!("a frame does not decode: {e}")))
}
