//! The port: read a file within limits.

use std::fmt;
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::time::Duration;

use crate::MediaInfo;

/// The service container key a reader is bound under; a PHP object implementing this interface replaces it.
pub const MEDIA_READER: &str = "SpeedySpec\\WpMedia\\MediaReader";

/// What to read: a path, or a descriptor already open (how a probe child receives its file).
#[derive(Debug)]
pub enum MediaSource {
    Path(PathBuf),
    Fd(OwnedFd),
}

/// How much a read may take. Uploads are untrusted, so every reader honours every bound it can.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeLimits {
    /// Bytes of the file a reader may read.
    pub max_bytes: u64,
    /// Wall time for the whole read.
    pub deadline: Duration,
    /// The largest single allocation the reader's library may make.
    pub max_alloc: u64,
    /// Streams a container may declare.
    pub max_streams: u32,
    /// Bytes of one embedded picture; larger pictures are dropped.
    pub max_picture_bytes: u64,
    /// Bytes of one tag value; larger values are dropped.
    pub max_tag_bytes: u64,
    /// Memory a probe child may use in all.
    pub max_memory: u64,
}

impl Default for ProbeLimits {
    fn default() -> ProbeLimits {
        ProbeLimits {
            max_bytes: 64 << 20,
            deadline: Duration::from_secs(5),
            max_alloc: 256 << 20,
            max_streams: 32,
            max_picture_bytes: 16 << 20,
            max_tag_bytes: 1 << 20,
            max_memory: 1 << 30,
        }
    }
}

/// Which bound a read ran into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Limit {
    Bytes,
    Streams,
    Memory,
    Cpu,
    Output,
}

/// Why a read produced no description (Special Case: WordPress's rules map each to `false` or a partial array).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReadError {
    NotFound,
    Unrecognized,
    Truncated,
    LimitExceeded(Limit),
    TimedOut,
    Io(String),
    Crashed(String),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::NotFound => f.write_str("the file does not exist"),
            ReadError::Unrecognized => f.write_str("the file is not a format the reader knows"),
            ReadError::Truncated => f.write_str("the file is truncated or malformed"),
            ReadError::LimitExceeded(limit) => {
                let name = match limit {
                    Limit::Bytes => "bytes",
                    Limit::Streams => "streams",
                    Limit::Memory => "memory",
                    Limit::Cpu => "cpu time",
                    Limit::Output => "output size",
                };
                write!(f, "reading the file exceeded its {name} limit")
            }
            ReadError::TimedOut => f.write_str("reading the file ran out of time"),
            ReadError::Io(what) => write!(f, "the file could not be read: {what}"),
            ReadError::Crashed(what) => write!(f, "the reader failed: {what}"),
        }
    }
}

impl std::error::Error for ReadError {}

/// Reads a media file's description (the port; FFmpeg, MIDI and PHP replacements are its adapters).
pub trait MediaReader {
    fn read(&self, source: MediaSource, limits: &ProbeLimits) -> Result<MediaInfo, ReadError>;
}
