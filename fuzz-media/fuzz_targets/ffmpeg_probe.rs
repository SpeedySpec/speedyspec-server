//! Any bytes through the FFmpeg shim, in-process, with a 1 MiB read budget and a 1 s deadline. Any result is fine; a
//! crash, hang or sanitizer report is a finding (minimize it into crates/speedyspec-wp-media-ffmpeg/tests/regressions).
#![no_main]

use std::io::Write;
#[cfg(target_os = "linux")]
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use std::sync::LazyLock;
use std::time::Duration;

use libfuzzer_sys::fuzz_target;
use speedyspec_wp_media_contracts::{MediaReader, MediaSource, ProbeLimits};
use speedyspec_wp_media_ffmpeg::FfmpegReader;

static LIMITS: LazyLock<ProbeLimits> = LazyLock::new(|| ProbeLimits {
    max_bytes: 1 << 20,
    deadline: Duration::from_secs(1),
    ..ProbeLimits::default()
});

/// The input as an anonymous in-memory file (Linux), so nothing touches the disk.
#[cfg(target_os = "linux")]
fn file_of(data: &[u8]) -> OwnedFd {
    // SAFETY: a valid NUL-terminated name; the descriptor is owned below.
    let raw = unsafe { libc::memfd_create(c"fuzz".as_ptr(), libc::MFD_CLOEXEC) };
    assert!(raw >= 0, "memfd_create failed");
    // SAFETY: just created and owned by nothing else.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    std::fs::File::from(fd.try_clone().unwrap()).write_all(data).unwrap();
    fd
}

/// Elsewhere, an unlinked temporary file.
#[cfg(not(target_os = "linux"))]
fn file_of(data: &[u8]) -> OwnedFd {
    let path = std::env::temp_dir().join(format!("speedyspec-fuzz-{}", std::process::id()));
    let mut file = std::fs::File::options().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    file.write_all(data).unwrap();
    OwnedFd::from(file)
}

fuzz_target!(|data: &[u8]| {
    let _ = FfmpegReader::new(&LIMITS).read(MediaSource::Fd(file_of(data)), &LIMITS);
});
