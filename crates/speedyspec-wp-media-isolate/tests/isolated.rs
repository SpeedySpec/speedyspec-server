//! IsolatedReader: reads in a limited child, killed at its deadline. The probe binary is built with `test-modes`.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use speedyspec_wp_media_contracts::{Limit, MediaReader, MediaSource, ProbeLimits, ReadError};
use speedyspec_wp_media_ffmpeg::FfmpegReader;
use speedyspec_wp_media_isolate::{IsolatedReader, ProbeCommand};
use speedyspec_wp_media_testkit as kit;

fn reader(extra: &[&str]) -> IsolatedReader {
    IsolatedReader::new(ProbeCommand {
        program: PathBuf::from(env!("CARGO_BIN_EXE_speedyspec-media-probe")),
        args: extra.iter().map(OsString::from).collect(),
    })
}

fn wav() -> tempfile::NamedTempFile {
    kit::write_temp(&kit::wav_pcm16(1.0, 8000, 1), "wav")
}

#[test]
fn reads_through_the_child() {
    let file = wav();
    let limits = ProbeLimits::default();
    let isolated = reader(&[])
        .read(MediaSource::Path(file.path().to_path_buf()), &limits)
        .unwrap();
    let local = FfmpegReader::new(&limits)
        .read(MediaSource::Path(file.path().to_path_buf()), &limits)
        .unwrap();
    assert_eq!(isolated, local);
}

#[test]
fn reads_midi_through_the_child() {
    let file = kit::write_temp(&kit::midi(500_000, 480, 960, "x"), "mid");
    let info = reader(&[])
        .read(
            MediaSource::Path(file.path().to_path_buf()),
            &ProbeLimits::default(),
        )
        .unwrap();
    assert_eq!(info.duration, Some(Duration::from_secs(1)));
}

#[test]
fn deadline_kills_a_loop() {
    let file = wav();
    let limits = ProbeLimits {
        deadline: Duration::from_millis(300),
        ..ProbeLimits::default()
    };
    let started = Instant::now();
    let result =
        reader(&["--simulate", "loop"]).read(MediaSource::Path(file.path().to_path_buf()), &limits);
    assert_eq!(result, Err(ReadError::TimedOut));
    assert!(
        started.elapsed() < Duration::from_millis(800),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn memory_limit_stops_runaway_allocation() {
    let file = wav();
    let limits = ProbeLimits {
        max_memory: 256 << 20,
        ..ProbeLimits::default()
    };
    let result = reader(&["--simulate", "alloc"])
        .read(MediaSource::Path(file.path().to_path_buf()), &limits);
    assert_eq!(result, Err(ReadError::LimitExceeded(Limit::Memory)));
}

#[test]
fn crash_is_reported() {
    let file = wav();
    let result = reader(&["--simulate", "crash"]).read(
        MediaSource::Path(file.path().to_path_buf()),
        &ProbeLimits::default(),
    );
    assert!(matches!(result, Err(ReadError::Crashed(_))), "{result:?}");
}

#[test]
fn garbage_output_is_crashed() {
    let file = wav();
    let result = reader(&["--simulate", "garbage"]).read(
        MediaSource::Path(file.path().to_path_buf()),
        &ProbeLimits::default(),
    );
    assert!(matches!(result, Err(ReadError::Crashed(_))), "{result:?}");
}

#[test]
fn symlink_and_fifo_are_io() {
    let file = wav();
    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("link.wav");
    std::os::unix::fs::symlink(file.path(), &link).unwrap();
    let result = reader(&[]).read(MediaSource::Path(link), &ProbeLimits::default());
    assert!(matches!(result, Err(ReadError::Io(_))), "{result:?}");
    let fifo = dir.path().join("pipe");
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    let result = reader(&[]).read(MediaSource::Path(fifo), &ProbeLimits::default());
    assert!(matches!(result, Err(ReadError::Io(_))), "{result:?}");
}

#[cfg(target_os = "linux")]
#[test]
fn seccomp_allows_the_fixtures() {
    for (name, bytes) in kit::all() {
        let file = kit::write_temp(&bytes, name.rsplit('.').next().unwrap());
        let result = reader(&[]).read(
            MediaSource::Path(file.path().to_path_buf()),
            &ProbeLimits::default(),
        );
        assert!(result.is_ok(), "{name}: {result:?}");
    }
}

/// A descriptor the worker opened without close-on-exec (PHP's files and sockets) never reaches the child.
#[test]
fn inherited_descriptors_are_closed_in_the_child() {
    let file = kit::write_temp(&kit::wav_pcm16(1.0, 8000, 1), "wav");
    let c = std::ffi::CString::new(file.path().to_str().unwrap()).unwrap();
    let leaky = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY) };
    assert!(leaky >= 3);
    let probe = reader(&["--simulate", "fds"]);
    let result = probe.read(
        MediaSource::Path(file.path().to_path_buf()),
        &ProbeLimits::default(),
    );
    unsafe { libc::close(leaky) };
    // The child exits 10 + the number of descriptors above stderr it can see.
    let detail = format!("{result:?}");
    assert!(detail.contains("exit Some(10)"), "{detail}");
}
