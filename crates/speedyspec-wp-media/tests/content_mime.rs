//! The content-MIME rule: fileinfo's answer, which the media reader may only confirm when fileinfo files an audio or
//! video file under the other kind.

use std::cell::Cell;
use std::path::Path;

use speedyspec_wp_media::mime::content_mime;
use speedyspec_wp_media_contracts::{
    AudioStream, Codec, ContainerKind, ContentSniffer, MediaInfo, MediaReader, MediaSource,
    Picture, ProbeLimits, RawViews, ReadError, Stream, TagSystem, VideoStream,
};

struct Fileinfo(Option<&'static str>);
impl ContentSniffer for Fileinfo {
    fn sniff(&self, _: &Path) -> Option<String> {
        self.0.map(str::to_owned)
    }
}

/// A reader that answers `result` and counts its reads.
struct Reader {
    result: Result<MediaInfo, ReadError>,
    reads: Cell<u32>,
}
impl Reader {
    fn new(result: Result<MediaInfo, ReadError>) -> Reader {
        Reader {
            result,
            reads: Cell::new(0),
        }
    }
}
impl MediaReader for Reader {
    fn read(&self, _: MediaSource, _: &ProbeLimits) -> Result<MediaInfo, ReadError> {
        self.reads.set(self.reads.get() + 1);
        self.result.clone()
    }
}

/// A reader the rule must not call.
struct Never;
impl MediaReader for Never {
    fn read(&self, _: MediaSource, _: &ProbeLimits) -> Result<MediaInfo, ReadError> {
        panic!("the reader was consulted");
    }
}

fn audio() -> Stream {
    Stream::Audio(AudioStream {
        codec: Codec::Vorbis,
        sample_rate: Some(44100),
        channels: Some(2),
        channel_layout: None,
        bits_per_sample: None,
        lossless: Some(false),
        bitrate: None,
        bitrate_mode: None,
        encoder: None,
    })
}

fn video() -> Stream {
    Stream::Video(VideoStream {
        codec: Codec::H264,
        width: Some(160),
        height: Some(120),
        frame_rate: None,
        bitrate: None,
        lossless: Some(false),
        encoder: None,
    })
}

fn info(mime: Option<&str>, streams: Vec<Stream>, pictures: Vec<Picture>) -> MediaInfo {
    MediaInfo {
        container: ContainerKind::Matroska,
        mime_type: mime.map(str::to_owned),
        duration: None,
        bitrate: None,
        file_size: None,
        created_at: None,
        streams,
        tags: vec![],
        pictures,
        raw: RawViews::default(),
    }
}

fn sniff(
    claimed: &str,
    fileinfo: Option<&'static str>,
    reader: &dyn MediaReader,
) -> Option<String> {
    content_mime(
        Path::new("/upload"),
        claimed,
        &Fileinfo(fileinfo),
        reader,
        &ProbeLimits::default(),
    )
}

#[test]
fn fileinfo_answer_when_it_agrees() {
    assert_eq!(
        sniff("audio/mpeg", Some("audio/mpeg"), &Never).as_deref(),
        Some("audio/mpeg")
    );
    assert_eq!(
        sniff("video/mp4", Some("application/octet-stream"), &Never).as_deref(),
        Some("application/octet-stream")
    );
}

#[test]
fn reader_rescues_audio_only_matroska() {
    let reader = Reader::new(Ok(info(Some("video/x-matroska"), vec![audio()], vec![])));
    assert_eq!(
        sniff("audio/x-matroska", Some("video/x-matroska"), &reader).as_deref(),
        Some("audio/x-matroska")
    );
    assert_eq!(reader.reads.get(), 1);
}

#[test]
fn reader_rescues_video_filed_as_audio() {
    let reader = Reader::new(Ok(info(Some("video/mp4"), vec![video(), audio()], vec![])));
    assert_eq!(
        sniff("video/3gpp", Some("audio/3gpp"), &reader).as_deref(),
        Some("video/mp4")
    );
}

#[test]
fn reader_never_overrides_non_media_fileinfo() {
    assert_eq!(
        sniff("audio/mpeg", Some("text/html"), &Never).as_deref(),
        Some("text/html")
    );
    assert_eq!(
        sniff("video/mp4", Some("image/png"), &Never).as_deref(),
        Some("image/png")
    );
}

#[test]
fn reader_failure_keeps_fileinfo() {
    for error in [
        ReadError::NotFound,
        ReadError::Unrecognized,
        ReadError::Truncated,
        ReadError::LimitExceeded(speedyspec_wp_media_contracts::Limit::Bytes),
        ReadError::TimedOut,
        ReadError::Io("gone".into()),
        ReadError::Crashed("signal 9".into()),
    ] {
        let reader = Reader::new(Err(error.clone()));
        assert_eq!(
            sniff("audio/x-ms-wma", Some("video/x-ms-asf"), &reader).as_deref(),
            Some("video/x-ms-asf"),
            "{error:?}"
        );
    }
}

#[test]
fn reader_without_the_claimed_kind_keeps_fileinfo() {
    // Claimed audio, but the file has video: fileinfo was right.
    let reader = Reader::new(Ok(info(None, vec![video(), audio()], vec![])));
    assert_eq!(
        sniff("audio/x-matroska", Some("video/x-matroska"), &reader).as_deref(),
        Some("video/x-matroska")
    );
    // Claimed video, but only audio.
    let reader = Reader::new(Ok(info(None, vec![audio()], vec![])));
    assert_eq!(
        sniff("video/x-ms-asf", Some("audio/x-ms-wma"), &reader).as_deref(),
        Some("audio/x-ms-wma")
    );
}

#[test]
fn attached_picture_is_not_video() {
    let cover = Picture {
        data: vec![1, 2, 3],
        mime: "image/png".into(),
        width: Some(48),
        height: Some(64),
        kind: None,
        system: TagSystem::Vorbis,
    };
    let reader = Reader::new(Ok(info(Some("audio/x-ms-wma"), vec![audio()], vec![cover])));
    assert_eq!(
        sniff("audio/x-ms-wma", Some("video/x-ms-asf"), &reader).as_deref(),
        Some("audio/x-ms-wma")
    );
}

#[test]
fn subtype_falls_back_to_the_claim() {
    let reader = Reader::new(Ok(info(None, vec![audio()], vec![])));
    assert_eq!(
        sniff("audio/x-matroska", Some("video/x-matroska"), &reader).as_deref(),
        Some("audio/x-matroska")
    );
}

#[test]
fn no_fileinfo_answer_is_none() {
    assert_eq!(sniff("audio/mpeg", None, &Never), None);
}

#[test]
fn non_media_claim_never_reads() {
    assert_eq!(
        sniff("application/pdf", Some("video/mp4"), &Never).as_deref(),
        Some("video/mp4")
    );
}
