use std::time::{Duration, UNIX_EPOCH};

use speedyspec_wp_media_contracts::{
    AudioStream, BitrateMode, Codec, ContainerKind, ImageKind, ImageStream, Limit, MediaInfo,
    Picture, RawViews, ReadError, Stream, Tag, TagKey, TagSystem, Tree, VideoStream,
};
use speedyspec_wp_media_isolate::wire::{self, WireError};

fn everything() -> MediaInfo {
    MediaInfo {
        container: ContainerKind::Image(ImageKind::Other("qoi".into())),
        mime_type: Some("video/mp4".into()),
        duration: Some(Duration::new(12, 345_678_901)),
        bitrate: Some(128_000),
        file_size: Some(1 << 40),
        created_at: Some(UNIX_EPOCH + Duration::new(1_714_979_289, 500)),
        streams: vec![
            Stream::Audio(AudioStream {
                codec: Codec::Pcm {
                    bits: 24,
                    float: false,
                },
                sample_rate: Some(48000),
                channels: Some(6),
                channel_layout: Some("5.1".into()),
                bits_per_sample: Some(24),
                lossless: Some(true),
                bitrate: None,
                bitrate_mode: Some(BitrateMode::Variable),
                encoder: Some("x".into()),
            }),
            Stream::Video(VideoStream {
                codec: Codec::Wmv("wmv3".into()),
                width: Some(1920),
                height: Some(1080),
                frame_rate: Some((30000, 1001)),
                bitrate: Some(5_000_000),
                lossless: Some(false),
                encoder: None,
            }),
            Stream::Image(ImageStream {
                kind: ImageKind::Avif,
                width: Some(1),
                height: Some(2),
                bit_depth: Some(10),
                channels: Some(4),
                animated: true,
                lossless: None,
            }),
            Stream::Data {
                codec: Codec::Other("midi".into()),
            },
        ],
        tags: vec![
            Tag {
                key: TagKey::Title,
                system: TagSystem::Id3v2,
                values: vec!["T".into(), "U".into()],
            },
            Tag {
                key: TagKey::Other("TXXX:mood".into()),
                system: TagSystem::Unknown,
                values: vec![String::new()],
            },
        ],
        pictures: vec![Picture {
            data: vec![0, 1, 2, 255],
            mime: "image/png".into(),
            width: Some(3),
            height: None,
            kind: Some("Cover (front)".into()),
            system: TagSystem::Vorbis,
        }],
        raw: RawViews {
            getid3: Some(Tree::Map(vec![(
                "a".into(),
                Tree::List(vec![Tree::Null, Tree::Bool(true)]),
            )])),
            reader: Some((
                "ffmpeg".into(),
                Tree::Map(vec![
                    ("f".into(), Tree::Float(1.5)),
                    ("i".into(), Tree::Int(-3)),
                    ("b".into(), Tree::Bytes(vec![9])),
                    ("s".into(), Tree::Str("x".into())),
                ]),
            )),
        },
    }
}

#[test]
fn round_trips_every_field() {
    let info = everything();
    assert_eq!(wire::decode(&wire::encode(&Ok(info.clone()))), Ok(Ok(info)));
}

#[test]
fn round_trips_errors() {
    for err in [
        ReadError::NotFound,
        ReadError::Unrecognized,
        ReadError::Truncated,
        ReadError::LimitExceeded(Limit::Bytes),
        ReadError::LimitExceeded(Limit::Streams),
        ReadError::LimitExceeded(Limit::Memory),
        ReadError::LimitExceeded(Limit::Cpu),
        ReadError::LimitExceeded(Limit::Output),
        ReadError::TimedOut,
        ReadError::Io("x".into()),
        ReadError::Crashed("y".into()),
    ] {
        assert_eq!(wire::decode(&wire::encode(&Err(err.clone()))), Ok(Err(err)));
    }
}

#[test]
fn rejects_other_versions() {
    let mut bytes = wire::encode(&Err(ReadError::NotFound));
    bytes[0] = 99;
    assert_eq!(wire::decode(&bytes), Err(WireError::Version(99)));
    assert!(matches!(wire::decode(&[1]), Err(WireError::Malformed(_))));
}

#[test]
fn a_deeply_nested_tree_is_refused_not_a_crash() {
    let mut tree = Tree::Int(1);
    for _ in 0..200 {
        tree = Tree::List(vec![tree]);
    }
    let info = MediaInfo {
        raw: RawViews {
            getid3: None,
            reader: Some(("x".into(), tree)),
        },
        ..everything()
    };
    let bytes = wire::encode(&Ok(info));
    assert!(matches!(wire::decode(&bytes), Err(WireError::Malformed(_))));
}

#[test]
fn hostile_nesting_bytes_do_not_overflow_the_stack() {
    // What a compromised child could write: a million nested one-element lists, in the wire's own node encoding.
    let mut bytes = wire::encode(&Err(ReadError::NotFound));
    bytes.truncate(2);
    let nested = wire::hostile_nesting_for_tests(1_000_000);
    bytes.extend_from_slice(&nested);
    assert!(wire::decode(&bytes).is_err());
}
