//! WordPress's audio and video metadata rules, through doubles.

use std::time::{Duration, UNIX_EPOCH};

use speedyspec_wp_media::functions::{
    add_id3_tag_data, media_creation_timestamp, read_audio_metadata, read_video_metadata,
};
use speedyspec_wp_media_contracts::{
    AudioStream, Codec, ContainerKind, MediaInfo, MediaReader, MediaSource, Picture, ProbeLimits,
    RawViews, ReadError, Stream, Tag, TagKey, TagSanitizer, TagSystem, TimeParser, Tree,
    VideoStream,
};

struct Fixed(Result<MediaInfo, ReadError>);
impl MediaReader for Fixed {
    fn read(&self, _: MediaSource, _: &ProbeLimits) -> Result<MediaInfo, ReadError> {
        self.0.clone()
    }
}

/// Marks what kses saw, so tests can tell values went through it.
struct Marking;
impl TagSanitizer for Marking {
    fn post(&self, value: &[u8]) -> Vec<u8> {
        [value, b"!"].concat()
    }
    fn post_deep(&self, value: &Tree) -> Tree {
        match value {
            Tree::Bytes(b) => Tree::Bytes([&b[..], b"!"].concat()),
            other => other.clone(),
        }
    }
}

struct Iso;
impl TimeParser for Iso {
    fn strtotime(&self, text: &[u8]) -> Option<i64> {
        (text == b"2024-05-06T07:08:09Z").then_some(1_714_979_289)
    }
}

fn b(s: &str) -> Tree {
    Tree::Bytes(s.as_bytes().to_vec())
}

fn map(entries: &[(&str, Tree)]) -> Tree {
    Tree::Map(
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    )
}

fn keys(t: &Tree) -> Vec<String> {
    match t {
        Tree::Map(m) => m.iter().map(|(k, _)| k.clone()).collect(),
        _ => vec![],
    }
}

fn audio_stream() -> Stream {
    Stream::Audio(AudioStream {
        codec: Codec::Mp3,
        sample_rate: Some(44100),
        channels: Some(2),
        channel_layout: None,
        bits_per_sample: None,
        lossless: Some(false),
        bitrate: Some(128_000),
        bitrate_mode: None,
        encoder: None,
    })
}

fn mp3(tags: Vec<Tag>, pictures: Vec<Picture>) -> MediaInfo {
    MediaInfo {
        container: ContainerKind::Mp3,
        mime_type: Some("audio/mpeg".into()),
        duration: Some(Duration::from_millis(2011)),
        bitrate: Some(128_000),
        file_size: Some(33_062),
        created_at: None,
        streams: vec![audio_stream()],
        tags,
        pictures,
        raw: RawViews::default(),
    }
}

fn existing() -> tempfile::NamedTempFile {
    speedyspec_wp_media_testkit::write_temp(b"x", "bin")
}

fn audio(info: Result<MediaInfo, ReadError>) -> Tree {
    let file = existing();
    read_audio_metadata(
        file.path(),
        &Fixed(info),
        &ProbeLimits::default(),
        &Marking,
        &Iso,
    )
    .unwrap()
    .metadata
}

#[test]
fn audio_keys_in_wordpress_order() {
    let m = audio(Ok(mp3(vec![], vec![])));
    assert_eq!(
        keys(&m),
        [
            "dataformat",
            "sample_rate",
            "channels",
            "bitrate",
            "lossless",
            "channelmode",
            "fileformat",
            "filesize",
            "mime_type",
            "length",
            "length_formatted"
        ]
    );
    assert_eq!(m.get("length"), Some(&Tree::Int(2)));
    assert_eq!(m.get("length_formatted"), Some(&b("0:02")));
}

#[test]
fn video_keys_in_wordpress_order() {
    let mut info = mp3(vec![], vec![]);
    info.container = ContainerKind::Mp4;
    info.streams = vec![
        Stream::Video(VideoStream {
            codec: Codec::H264,
            width: Some(160),
            height: Some(120),
            frame_rate: Some((10, 1)),
            bitrate: Some(89_450),
            lossless: Some(false),
            encoder: None,
        }),
        audio_stream(),
    ];
    let file = existing();
    let out = read_video_metadata(
        file.path(),
        &Fixed(Ok(info)),
        &ProbeLimits::default(),
        &Marking,
        &Iso,
    )
    .unwrap();
    assert_eq!(
        keys(&out.metadata),
        [
            "lossless",
            "bitrate",
            "filesize",
            "mime_type",
            "length",
            "length_formatted",
            "width",
            "height",
            "fileformat",
            "dataformat",
            "codec",
            "audio",
            "created_timestamp"
        ]
    );
    assert_eq!(out.file_format, Some(b"mp4".to_vec()));
    assert_eq!(
        out.metadata.get("created_timestamp"),
        Some(&Tree::Int(-2_082_844_800))
    );
    assert!(
        out.metadata
            .get("audio")
            .and_then(|a| a.get("streams"))
            .is_none()
    );
}

#[test]
fn missing_file_is_none() {
    let out = read_audio_metadata(
        std::path::Path::new("/nonexistent/x.mp3"),
        &Fixed(Ok(mp3(vec![], vec![]))),
        &ProbeLimits::default(),
        &Marking,
        &Iso,
    );
    assert!(out.is_none());
}

#[test]
fn empty_values_are_skipped_like_php() {
    let mut info = mp3(vec![], vec![]);
    info.duration = Some(Duration::ZERO);
    let m = audio(Ok(info));
    assert!(m.get("length").is_none());
}

#[test]
fn unreadable_file_gives_filesize_only() {
    let m = audio(Err(ReadError::Unrecognized));
    assert_eq!(keys(&m), ["filesize"]);
}

fn tag(key: TagKey, system: TagSystem, value: &str) -> Tag {
    Tag {
        key,
        system,
        values: vec![value.into()],
    }
}

#[test]
fn id3v2_wins_over_id3v1() {
    let m = audio(Ok(mp3(
        vec![
            tag(TagKey::Title, TagSystem::Id3v2, "Two"),
            tag(TagKey::Artist, TagSystem::Id3v1, "One"),
        ],
        vec![],
    )));
    assert_eq!(m.get("title"), Some(&b("Two!")));
    assert!(m.get("artist").is_none());
}

#[test]
fn length_key_is_skipped() {
    let mut meta = map(&[]);
    let data = map(&[(
        "id3v2",
        map(&[(
            "comments",
            map(&[
                ("length", Tree::List(vec![b("9")])),
                ("title", Tree::List(vec![b("T")])),
            ]),
        )]),
    )]);
    add_id3_tag_data(&mut meta, &data, &Marking);
    assert_eq!(keys(&meta), ["title"]);
}

#[test]
fn terms_of_use_fix() {
    let mut meta = map(&[]);
    let data = map(&[(
        "id3v2",
        map(&[(
            "comments",
            map(&[("terms_of_use", Tree::List(vec![b("yright notice.")]))]),
        )]),
    )]);
    add_id3_tag_data(&mut meta, &data, &Marking);
    assert_eq!(meta.get("terms_of_use"), Some(&b("Copyright notice.!")));
}

#[test]
fn apic_image_has_dimensions() {
    let picture = Picture {
        data: vec![1, 2, 3],
        mime: "image/jpeg".into(),
        width: Some(64),
        height: Some(48),
        kind: None,
        system: TagSystem::Id3v2,
    };
    let m = audio(Ok(mp3(
        vec![tag(TagKey::Title, TagSystem::Id3v2, "T")],
        vec![picture],
    )));
    let image = m.get("image").unwrap();
    assert_eq!(keys(image), ["data", "mime", "width", "height"]);
    assert_eq!(image.get("width"), Some(&Tree::Int(64)));
}

#[test]
fn picture_image_has_data_and_mime_only() {
    let mut info = mp3(
        vec![],
        vec![Picture {
            data: vec![7],
            mime: "image/png".into(),
            width: Some(1),
            height: Some(1),
            kind: None,
            system: TagSystem::Vorbis,
        }],
    );
    info.container = ContainerKind::Flac;
    let m = audio(Ok(info));
    assert_eq!(keys(m.get("image").unwrap()), ["data", "mime"]);
}

#[test]
fn tags_from_other_formats_when_no_id3() {
    let mut info = mp3(
        vec![tag(TagKey::Title, TagSystem::Vorbis, "Vorbis Title")],
        vec![],
    );
    info.container = ContainerKind::Flac;
    let m = audio(Ok(info));
    assert_eq!(m.get("title"), Some(&b("Vorbis Title!")));
    assert!(m.get("picture").is_none());
}

#[test]
fn id3_present_ignores_other_formats() {
    let mut meta = map(&[]);
    let data = map(&[
        (
            "id3v1",
            map(&[("comments", map(&[("title", Tree::List(vec![b("V1")]))]))]),
        ),
        (
            "comments",
            map(&[("artist", Tree::List(vec![b("Merged")]))]),
        ),
    ]);
    add_id3_tag_data(&mut meta, &data, &Marking);
    assert_eq!(keys(&meta), ["title"]);
}

#[test]
fn creation_timestamp_per_format() {
    let asf = map(&[
        ("fileformat", b("asf")),
        (
            "asf",
            map(&[(
                "file_properties_object",
                map(&[("creation_date_unix", Tree::Float(12.9))]),
            )]),
        ),
    ]);
    assert_eq!(media_creation_timestamp(&asf, &Iso), Some(12));
    let mkv = map(&[
        ("fileformat", b("matroska")),
        (
            "matroska",
            map(&[(
                "comments",
                map(&[("creation_time", Tree::List(vec![b("2024-05-06T07:08:09Z")]))]),
            )]),
        ),
    ]);
    assert_eq!(media_creation_timestamp(&mkv, &Iso), Some(1_714_979_289));
    let webm = map(&[
        ("fileformat", b("webm")),
        (
            "matroska",
            map(&[("info", Tree::List(vec![map(&[("DateUTC_unix", b("77"))])]))]),
        ),
    ]);
    assert_eq!(media_creation_timestamp(&webm, &Iso), Some(77));
    let mov = map(&[
        ("fileformat", b("quicktime")),
        (
            "quicktime",
            map(&[(
                "moov",
                map(&[(
                    "subatoms",
                    map(&[("0", map(&[("creation_time_unix", Tree::Int(5))]))]),
                )]),
            )]),
        ),
    ]);
    assert_eq!(media_creation_timestamp(&mov, &Iso), Some(5));
    let _ = UNIX_EPOCH;
}

#[test]
fn unknown_format_has_no_timestamp() {
    assert_eq!(
        media_creation_timestamp(&map(&[("fileformat", b("mp3"))]), &Iso),
        None
    );
    assert_eq!(media_creation_timestamp(&map(&[]), &Iso), None);
}

#[test]
fn non_utf8_tags_pass_as_bytes() {
    let mut meta = map(&[]);
    let data = map(&[(
        "id3v1",
        map(&[(
            "comments",
            map(&[("title", Tree::List(vec![Tree::Bytes(b"Caf\xe9".to_vec())]))]),
        )]),
    )]);
    add_id3_tag_data(&mut meta, &data, &Marking);
    assert_eq!(meta.get("title"), Some(&Tree::Bytes(b"Caf\xe9!".to_vec())));
}

#[test]
fn speedyspec_key_carries_the_neutral_view() {
    let file = existing();
    let out = read_audio_metadata(
        file.path(),
        &Fixed(Ok(mp3(vec![], vec![]))),
        &ProbeLimits::default(),
        &Marking,
        &Iso,
    )
    .unwrap();
    let neutral = out.data.get("speedyspec").unwrap();
    assert_eq!(neutral.get("schema"), Some(&Tree::Int(1)));
    assert_eq!(neutral.get("container"), Some(&b("mp3")));
}
