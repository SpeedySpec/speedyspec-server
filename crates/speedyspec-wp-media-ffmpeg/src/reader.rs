//! The FFmpeg reader: opens the file safely, asks the shim, and maps the answer.

use std::ffi::CString;
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::Once;

use speedyspec_wp_media_contracts::{Limit, MediaInfo, MediaReader, MediaSource, ProbeLimits, ReadError};

use crate::ffi;
use crate::probe;

/// Reads media in this process through the shim (Adapter over libavformat). Untrusted files should go through the
/// isolating proxy instead, which runs this reader in a limited child.
pub struct FfmpegReader;

static INIT: Once = Once::new();

impl FfmpegReader {
    /// The reader; the first one made sets FFmpeg's per-allocation cap from `limits` and silences its log.
    pub fn new(limits: &ProbeLimits) -> FfmpegReader {
        let max_alloc = i64::try_from(limits.max_alloc).unwrap_or(i64::MAX);
        // SAFETY: plain setters, run once per process before any probe.
        INIT.call_once(|| unsafe { ffi::ss_global_init(max_alloc) });
        FfmpegReader
    }
}

/// Opens `path` for reading without following a final symlink or blocking on a FIFO.
pub(crate) fn open_untrusted(path: &Path) -> Result<OwnedFd, ReadError> {
    let c = CString::new(path.as_os_str().as_bytes()).map_err(|_| ReadError::Io("the path contains NUL".into()))?;
    // SAFETY: a valid C string; the flags only restrict what open does.
    let fd = unsafe {
        libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
    };
    if fd < 0 {
        let err = std::io::Error::last_os_error();
        return Err(match err.raw_os_error() {
            Some(libc::ENOENT) | Some(libc::ENOTDIR) => ReadError::NotFound,
            Some(libc::ELOOP) => ReadError::Io("the path is a symlink".into()),
            _ => ReadError::Io(err.to_string()),
        });
    }
    // SAFETY: `fd` was just opened and is owned by nothing else.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Fails unless `fd` is a regular file; returns its size.
pub(crate) fn regular_size(fd: &OwnedFd) -> Result<u64, ReadError> {
    let file = File::from(fd.try_clone().map_err(|e| ReadError::Io(e.to_string()))?);
    let meta = file.metadata().map_err(|e| ReadError::Io(e.to_string()))?;
    if !meta.file_type().is_file() {
        return Err(ReadError::Io("not a regular file".into()));
    }
    Ok(meta.len())
}

/// Whether the file has an ID3v2 header and an ID3v1 trailer, read with `pread` so the offset is untouched.
fn id3_presence(fd: &OwnedFd, size: u64) -> (bool, bool) {
    let Ok(clone) = fd.try_clone() else {
        return (false, false);
    };
    let file = File::from(clone);
    let mut head = [0u8; 3];
    let v2 = file.read_exact_at(&mut head, 0).is_ok() && &head == b"ID3";
    let mut tail = [0u8; 3];
    let v1 = size >= 128 && file.read_exact_at(&mut tail, size - 128).is_ok() && &tail == b"TAG";
    (v2, v1)
}

struct Guard(*mut ffi::SsResult);

impl Drop for Guard {
    fn drop(&mut self) {
        // SAFETY: the pointer came from ss_probe and is freed exactly once.
        unsafe { ffi::ss_free(self.0) };
    }
}

impl MediaReader for FfmpegReader {
    fn read(&self, source: MediaSource, limits: &ProbeLimits) -> Result<MediaInfo, ReadError> {
        let fd = match source {
            MediaSource::Path(path) => open_untrusted(&path)?,
            MediaSource::Fd(fd) => fd,
        };
        let size = regular_size(&fd)?;
        let shim_limits = ffi::SsLimits {
            max_bytes: i64::try_from(limits.max_bytes).unwrap_or(i64::MAX),
            deadline_ms: i64::try_from(limits.deadline.as_millis()).unwrap_or(i64::MAX),
            max_streams: i32::try_from(limits.max_streams).unwrap_or(i32::MAX),
            max_picture_bytes: i64::try_from(limits.max_picture_bytes).unwrap_or(i64::MAX),
            max_tag_bytes: i64::try_from(limits.max_tag_bytes).unwrap_or(i64::MAX),
        };
        let mut out: *mut ffi::SsResult = std::ptr::null_mut();
        // SAFETY: `fd` is open for the call; `out` receives a result the guard frees.
        let status = unsafe { ffi::ss_probe(fd.as_raw_fd(), &shim_limits, &mut out) };
        let guard = Guard(out);
        let result = match status {
            ffi::SS_OK if !guard.0.is_null() => {
                // SAFETY: SS_OK with a non-null pointer means a complete result.
                probe::copy(unsafe { &*guard.0 })
            }
            ffi::SS_UNRECOGNIZED => return Err(ReadError::Unrecognized),
            ffi::SS_TRUNCATED => return Err(ReadError::Truncated),
            ffi::SS_LIMIT_BYTES => return Err(ReadError::LimitExceeded(Limit::Bytes)),
            ffi::SS_LIMIT_STREAMS => return Err(ReadError::LimitExceeded(Limit::Streams)),
            ffi::SS_TIMEOUT => return Err(ReadError::TimedOut),
            ffi::SS_IO => return Err(ReadError::Io("not a regular file".into())),
            ffi::SS_NOMEM => return Err(ReadError::LimitExceeded(Limit::Memory)),
            other => return Err(ReadError::Crashed(format!("shim status {other}"))),
        };
        drop(guard);
        let (id3v2, id3v1) = id3_presence(&fd, size);
        Ok(crate::mapper::map(probe::RawProbe { file_size: size, id3v2, id3v1, ..result }))
    }
}
