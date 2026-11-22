//! The FFmpeg reader in-process, on the test kit's files.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use speedyspec_wp_media_contracts::{Limit, MediaInfo, MediaReader, MediaSource, ProbeLimits, ReadError};
use speedyspec_wp_media_ffmpeg::FfmpegReader;
use speedyspec_wp_media_testkit as kit;

fn read_bytes(bytes: &[u8], ext: &str, limits: &ProbeLimits) -> Result<MediaInfo, ReadError> {
    let file = kit::write_temp(bytes, ext);
    FfmpegReader::new(limits).read(MediaSource::Path(file.path().to_path_buf()), limits)
}

fn read(bytes: &[u8], ext: &str) -> Result<MediaInfo, ReadError> {
    read_bytes(bytes, ext, &ProbeLimits::default())
}

fn close(actual: Option<Duration>, expected: f64, tolerance: f64) {
    let actual = actual.expect("a duration").as_secs_f64();
    assert!((actual - expected).abs() <= tolerance, "duration {actual}, expected {expected}");
}

#[test]
fn reads_wav_duration() {
    let info = read(&kit::wav_pcm16(2.0, 8000, 1), "wav").unwrap();
    close(info.duration, 2.0, 0.001);
    assert_eq!(info.streams.len(), 1);
}

#[test]
fn reads_mp3_duration() {
    let info = read(&kit::mp3_cbr(100), "mp3").unwrap();
    close(info.duration, 100.0 * 1152.0 / 44100.0, 0.03);
}

#[test]
fn reads_png() {
    let info = read(&kit::png(3, 2), "png").unwrap();
    assert_eq!(info.streams.len(), 1);
}

#[test]
fn unrecognized_bytes() {
    assert_eq!(read(&[0u8; 1024], "bin"), Err(ReadError::Unrecognized));
}

#[test]
fn truncated_header() {
    let wav = kit::wav_pcm16(1.0, 8000, 1);
    let result = read(&wav[..20], "wav");
    assert!(matches!(result, Err(ReadError::Truncated) | Err(ReadError::Unrecognized)), "{result:?}");
}

#[test]
fn byte_limit() {
    let limits = ProbeLimits { max_bytes: 4096, ..ProbeLimits::default() };
    let result = read_bytes(&kit::wav_pcm16(10.0, 44100, 2), "wav", &limits);
    assert_eq!(result, Err(ReadError::LimitExceeded(Limit::Bytes)));
}

#[test]
fn directory_is_io() {
    let dir = tempfile::tempdir().unwrap();
    let limits = ProbeLimits::default();
    let result = FfmpegReader::new(&limits).read(MediaSource::Path(dir.path().to_path_buf()), &limits);
    assert!(matches!(result, Err(ReadError::Io(_))), "{result:?}");
}

#[test]
fn fifo_is_io() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("pipe");
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    let limits = ProbeLimits::default();
    let started = Instant::now();
    let result = FfmpegReader::new(&limits).read(MediaSource::Path(fifo), &limits);
    assert!(matches!(result, Err(ReadError::Io(_))), "{result:?}");
    assert!(started.elapsed() < Duration::from_millis(100));
}

#[test]
fn symlink_is_refused() {
    let file = kit::write_temp(&kit::wav_pcm16(1.0, 8000, 1), "wav");
    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("link.wav");
    std::os::unix::fs::symlink(file.path(), &link).unwrap();
    let limits = ProbeLimits::default();
    let result = FfmpegReader::new(&limits).read(MediaSource::Path(link), &limits);
    assert!(matches!(result, Err(ReadError::Io(_))), "{result:?}");
}

#[test]
fn missing_is_not_found() {
    let limits = ProbeLimits::default();
    let result = FfmpegReader::new(&limits).read(MediaSource::Path(PathBuf::from("/nonexistent/x.wav")), &limits);
    assert_eq!(result, Err(ReadError::NotFound));
}
