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

use speedyspec_wp_media_contracts::{Codec, ContainerKind, ImageKind, Stream, TagKey, TagSystem};

fn tag(info: &MediaInfo, key: TagKey) -> Option<(&str, TagSystem)> {
    info.tags.iter().find(|t| t.key == key).map(|t| (t.values[0].as_str(), t.system))
}

#[test]
fn mp3_with_id3v2_tags_and_cover() {
    let png = kit::png(3, 2);
    let info = read(&kit::mp3_with_id3v2(40, "A Title", "An Artist", Some(&png)), "mp3").unwrap();
    assert_eq!(info.container, ContainerKind::Mp3);
    assert_eq!(info.mime_type.as_deref(), Some("audio/mpeg"));
    assert_eq!(tag(&info, TagKey::Title), Some(("A Title", TagSystem::Id3v2)));
    assert_eq!(tag(&info, TagKey::Artist), Some(("An Artist", TagSystem::Id3v2)));
    assert_eq!(info.pictures.len(), 1, "{:?}", info.pictures);
    let picture = &info.pictures[0];
    assert_eq!(picture.mime, "image/png");
    assert_eq!(picture.data, png);
    assert_eq!((picture.width, picture.height), (Some(3), Some(2)));
    assert!(info.streams.iter().all(|s| matches!(s, Stream::Audio(_))));
}

#[test]
fn mp3_with_id3v1_only() {
    let info = read(&kit::mp3_with_id3v1(40, "Old Title"), "mp3").unwrap();
    assert_eq!(tag(&info, TagKey::Title), Some(("Old Title", TagSystem::Id3v1)));
}

#[test]
fn wav_stream_properties() {
    let info = read(&kit::wav_pcm16(1.0, 22050, 2), "wav").unwrap();
    assert_eq!(info.container, ContainerKind::Wav);
    let Stream::Audio(audio) = &info.streams[0] else { panic!("{:?}", info.streams) };
    assert_eq!(audio.sample_rate, Some(22050));
    assert_eq!(audio.channels, Some(2));
    assert_eq!(audio.codec, Codec::Pcm { bits: 16, float: false });
    assert_eq!(audio.lossless, Some(true));
    assert_eq!(audio.bits_per_sample, Some(16));
}

#[test]
fn png_dimensions() {
    let info = read(&kit::png(3, 2), "png").unwrap();
    assert_eq!(info.container, ContainerKind::Image(ImageKind::Png));
    let Stream::Image(image) = &info.streams[0] else { panic!("{:?}", info.streams) };
    assert_eq!((image.width, image.height), (Some(3), Some(2)));
    assert_eq!((image.bit_depth, image.channels), (Some(8), Some(3)));
}

#[test]
fn gif_bmp_and_aiff() {
    let gif = read(&kit::gif(4, 3), "gif").unwrap();
    assert_eq!(gif.container, ContainerKind::Image(ImageKind::Gif));
    let bmp = read(&kit::bmp(4, 3), "bmp").unwrap();
    assert_eq!(bmp.container, ContainerKind::Image(ImageKind::Bmp));
    let Stream::Image(image) = &bmp.streams[0] else { panic!("{:?}", bmp.streams) };
    assert_eq!((image.width, image.height), (Some(4), Some(3)));
    let aiff = read(&kit::aiff_pcm16(1.0, 8000, 1), "aiff").unwrap();
    assert_eq!(aiff.container, ContainerKind::Aiff);
    close(aiff.duration, 1.0, 0.001);
}
