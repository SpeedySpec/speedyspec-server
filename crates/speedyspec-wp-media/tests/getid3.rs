//! The getID3-shaped view on hand-built MediaInfo values.

use std::time::{Duration, UNIX_EPOCH};

use speedyspec_wp_media::getid3::{getid3_view, playtime_string};
use speedyspec_wp_media_contracts::{
    AudioStream, Codec, ContainerKind, MediaInfo, Picture, RawViews, Stream, Tag, TagKey,
    TagSystem, Tree, VideoStream,
};

fn audio(codec: Codec, rate: u32, channels: u16) -> Stream {
    Stream::Audio(AudioStream {
        codec,
        sample_rate: Some(rate),
        channels: Some(channels),
        channel_layout: None,
        bits_per_sample: None,
        lossless: Some(false),
        bitrate: Some(128_000),
        bitrate_mode: None,
        encoder: None,
    })
}

fn info(container: ContainerKind, streams: Vec<Stream>) -> MediaInfo {
    MediaInfo {
        container,
        mime_type: None,
        duration: Some(Duration::from_millis(2011)),
        bitrate: Some(129_000),
        file_size: Some(35_066),
        created_at: None,
        streams,
        tags: vec![],
        pictures: vec![],
        raw: RawViews::default(),
    }
}

fn path<'a>(tree: &'a Tree, keys: &[&str]) -> Option<&'a Tree> {
    keys.iter()
        .try_fold(tree, |t, k| match (t, k.parse::<usize>()) {
            (Tree::List(items), Ok(i)) => items.get(i),
            _ => t.get(k),
        })
}

fn bytes(s: &str) -> Tree {
    Tree::Bytes(s.as_bytes().to_vec())
}

#[test]
fn mp3_view_has_id3v2_comments_and_apic() {
    let mut i = info(ContainerKind::Mp3, vec![audio(Codec::Mp3, 44100, 2)]);
    i.tags = vec![
        Tag {
            key: TagKey::Title,
            system: TagSystem::Id3v2,
            values: vec!["T".into()],
        },
        Tag {
            key: TagKey::Track,
            system: TagSystem::Id3v2,
            values: vec!["3".into()],
        },
    ];
    i.pictures = vec![Picture {
        data: vec![1, 2],
        mime: "image/jpeg".into(),
        width: Some(64),
        height: Some(48),
        kind: None,
        system: TagSystem::Id3v2,
    }];
    let v = getid3_view(&i);
    assert_eq!(path(&v, &["fileformat"]), Some(&bytes("mp3")));
    assert_eq!(path(&v, &["mime_type"]), Some(&bytes("audio/mpeg")));
    assert_eq!(path(&v, &["audio", "dataformat"]), Some(&bytes("mp3")));
    assert_eq!(path(&v, &["audio", "channelmode"]), Some(&bytes("stereo")));
    assert_eq!(
        path(&v, &["id3v2", "comments", "title", "0"]),
        Some(&bytes("T"))
    );
    assert_eq!(
        path(&v, &["id3v2", "comments", "track_number", "0"]),
        Some(&bytes("3"))
    );
    assert_eq!(
        path(&v, &["id3v2", "APIC", "0", "image_mime"]),
        Some(&bytes("image/jpeg"))
    );
    assert_eq!(
        path(&v, &["id3v2", "APIC", "0", "image_width"]),
        Some(&Tree::Int(64))
    );
    assert_eq!(
        path(&v, &["comments", "picture", "0", "data"]),
        Some(&Tree::Bytes(vec![1, 2]))
    );
}

#[test]
fn id3v1_comments_carry_its_five_fields() {
    let mut i = info(ContainerKind::Mp3, vec![audio(Codec::Mp3, 44100, 1)]);
    i.tags = vec![Tag {
        key: TagKey::Title,
        system: TagSystem::Id3v1,
        values: vec!["Only".into()],
    }];
    let v = getid3_view(&i);
    assert_eq!(
        path(&v, &["id3v1", "comments", "title", "0"]),
        Some(&bytes("Only"))
    );
    assert_eq!(
        path(&v, &["id3v1", "comments", "album", "0"]),
        Some(&bytes(""))
    );
    assert_eq!(
        path(&v, &["id3v1", "comments", "comment", "0"]),
        Some(&bytes(""))
    );
    assert_eq!(path(&v, &["audio", "channelmode"]), Some(&bytes("mono")));
}

#[test]
fn flac_view_has_merged_comments_and_picture() {
    let mut i = info(ContainerKind::Flac, vec![audio(Codec::Flac, 44100, 2)]);
    i.tags = vec![Tag {
        key: TagKey::Title,
        system: TagSystem::Vorbis,
        values: vec!["F".into()],
    }];
    i.pictures = vec![Picture {
        data: vec![9],
        mime: "image/png".into(),
        width: Some(48),
        height: Some(64),
        kind: None,
        system: TagSystem::Vorbis,
    }];
    let v = getid3_view(&i);
    assert_eq!(path(&v, &["comments", "title", "0"]), Some(&bytes("F")));
    assert_eq!(
        path(&v, &["comments", "picture", "0", "image_mime"]),
        Some(&bytes("image/png"))
    );
    assert_eq!(path(&v, &["id3v2"]), None);
}

#[test]
fn mp4_creation_time_is_under_quicktime_moov() {
    let mut i = info(ContainerKind::Mp4, vec![audio(Codec::Aac, 44100, 2)]);
    i.created_at = Some(UNIX_EPOCH + Duration::from_secs(1_714_979_289));
    let v = getid3_view(&i);
    assert_eq!(path(&v, &["fileformat"]), Some(&bytes("mp4")));
    assert_eq!(
        path(
            &v,
            &["quicktime", "moov", "subatoms", "0", "creation_time_unix"]
        ),
        Some(&Tree::Int(1_714_979_289))
    );
    // An ISO file without a creation time has a zero mvhd time: getID3 reports the 1904 epoch.
    i.created_at = None;
    let v = getid3_view(&i);
    assert_eq!(
        path(
            &v,
            &["quicktime", "moov", "subatoms", "0", "creation_time_unix"]
        ),
        Some(&Tree::Int(-2_082_844_800))
    );
}

#[test]
fn matroska_creation_time_is_unix() {
    let video = Stream::Video(VideoStream {
        codec: Codec::Vp9,
        width: Some(160),
        height: Some(120),
        frame_rate: Some((10, 1)),
        bitrate: None,
        lossless: None,
        encoder: None,
    });
    let mut i = info(
        ContainerKind::WebM,
        vec![video, audio(Codec::Opus, 48000, 1)],
    );
    i.created_at = Some(UNIX_EPOCH + Duration::from_secs(1_714_979_289));
    let v = getid3_view(&i);
    assert_eq!(path(&v, &["fileformat"]), Some(&bytes("webm")));
    assert_eq!(path(&v, &["video", "dataformat"]), Some(&bytes("V_VP9")));
    assert_eq!(path(&v, &["video", "resolution_x"]), Some(&Tree::Int(160)));
    assert_eq!(
        path(&v, &["matroska", "info", "0", "DateUTC_unix"]),
        Some(&Tree::Int(1_714_979_289))
    );
}

#[test]
fn asf_creation_time_is_under_file_properties() {
    let i = info(ContainerKind::Asf, vec![audio(Codec::WmaV2, 44100, 2)]);
    let v = getid3_view(&i);
    assert_eq!(path(&v, &["fileformat"]), Some(&bytes("asf")));
    assert_eq!(
        path(&v, &["asf", "file_properties_object", "creation_date_unix"]),
        Some(&Tree::Int(0))
    );
    assert_eq!(
        path(&v, &["audio", "codec"]),
        Some(&bytes("Windows Media Audio V8"))
    );
}

#[test]
fn playtime_string_formats() {
    assert_eq!(playtime_string(61.4), "1:01");
    assert_eq!(playtime_string(3600.6), "1:00:01");
    assert_eq!(playtime_string(0.4), "0:00");
    assert_eq!(playtime_string(2.011), "0:02");
}

#[test]
fn audio_streams_listed() {
    let i = info(
        ContainerKind::Wav,
        vec![audio(
            Codec::Pcm {
                bits: 16,
                float: false,
            },
            44100,
            2,
        )],
    );
    let v = getid3_view(&i);
    let Some(Tree::List(streams)) = path(&v, &["audio", "streams"]) else {
        panic!("streams")
    };
    assert_eq!(streams.len(), 1);
    assert_eq!(
        path(&v, &["audio", "codec"]),
        Some(&bytes("Pulse Code Modulation (PCM)"))
    );
    assert_eq!(path(&v, &["playtime_string"]), Some(&bytes("0:02")));
}

#[test]
fn neutral_round_trips() {
    use speedyspec_wp_media::neutral::{from_neutral, neutral};
    let mut i = info(
        ContainerKind::Mp4,
        vec![audio(
            Codec::Pcm {
                bits: 24,
                float: false,
            },
            48000,
            2,
        )],
    );
    i.tags = vec![Tag {
        key: TagKey::Other("mood".into()),
        system: TagSystem::Mp4,
        values: vec!["calm".into()],
    }];
    i.created_at = Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000));
    let back = from_neutral(&neutral(&i)).unwrap();
    assert_eq!(back.container, i.container);
    assert_eq!(back.streams, i.streams);
    assert_eq!(back.tags, i.tags);
    assert_eq!(back.created_at, i.created_at);
    assert!(from_neutral(&Tree::Map(vec![("schema".into(), Tree::Int(2))])).is_err());
}

#[test]
fn malformed_neutral_values_are_errors_not_panics() {
    use speedyspec_wp_media::neutral::from_neutral;
    let base = |stream: Tree, duration: Tree| {
        Tree::Map(vec![
            ("schema".into(), Tree::Int(1)),
            ("container".into(), Tree::Bytes(b"mp3".to_vec())),
            ("duration".into(), duration),
            ("streams".into(), Tree::List(vec![stream])),
        ])
    };
    let audio = |codec: &[u8]| {
        Tree::Map(vec![
            ("type".into(), Tree::Bytes(b"audio".to_vec())),
            ("codec".into(), Tree::Bytes(codec.to_vec())),
        ])
    };
    for codec in [&b"pcm_"[..], "pcm_é16".as_bytes(), b"pcm_\xff16"] {
        let result =
            std::panic::catch_unwind(|| from_neutral(&base(audio(codec), Tree::Float(1.0))));
        assert!(result.is_ok(), "codec {codec:?} panicked");
    }
    let result = std::panic::catch_unwind(|| from_neutral(&base(audio(b"mp3"), Tree::Float(1e20))));
    assert!(
        matches!(result, Ok(Ok(ref info)) if info.duration.is_none()),
        "{result:?}"
    );
}
