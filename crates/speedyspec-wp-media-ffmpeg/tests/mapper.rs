//! The Data Mapper on hand-built probes: no FFmpeg involved.

use std::time::{Duration, UNIX_EPOCH};

use speedyspec_wp_media_contracts::{
    Codec, ContainerKind, ImageKind, Stream, TagKey, TagSystem, Tree,
};
use speedyspec_wp_media_ffmpeg::{RawProbe, RawStream, map};

fn audio(codec: &str) -> RawStream {
    RawStream {
        media_type: 1,
        codec_name: codec.into(),
        sample_rate: 44100,
        channels: 2,
        ..RawStream::default()
    }
}

fn video(codec: &str) -> RawStream {
    RawStream {
        media_type: 0,
        codec_name: codec.into(),
        width: 640,
        height: 360,
        ..RawStream::default()
    }
}

fn probe(format: &str, tags: &[(&str, &str)], streams: Vec<RawStream>) -> RawProbe {
    RawProbe {
        format_name: format.into(),
        tags: tags
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        streams,
        ..RawProbe::default()
    }
}

const MOV: &str = "mov,mp4,m4a,3gp,3g2,mj2";

#[test]
fn mp4_audio_only_is_audio_mp4() {
    let info = map(probe(MOV, &[("major_brand", "M4A ")], vec![audio("aac")]));
    assert_eq!(info.container, ContainerKind::Mp4);
    assert_eq!(info.mime_type.as_deref(), Some("audio/mp4"));
    let info = map(probe(
        MOV,
        &[("major_brand", "isom")],
        vec![video("h264"), audio("aac")],
    ));
    assert_eq!(info.mime_type.as_deref(), Some("video/mp4"));
}

#[test]
fn quicktime_brand_is_quicktime() {
    let info = map(probe(MOV, &[("major_brand", "qt  ")], vec![video("h264")]));
    assert_eq!(info.container, ContainerKind::QuickTime);
    assert_eq!(info.mime_type.as_deref(), Some("video/quicktime"));
    let info = map(probe(MOV, &[("major_brand", "3gp5")], vec![video("h263")]));
    assert_eq!(info.container, ContainerKind::ThreeGp);
}

#[test]
fn avif_brand_is_an_image() {
    let info = map(probe(MOV, &[("major_brand", "avif")], vec![video("av1")]));
    assert_eq!(info.container, ContainerKind::Image(ImageKind::Avif));
    assert_eq!(info.mime_type.as_deref(), Some("image/avif"));
    assert!(matches!(info.streams[0], Stream::Image(_)));
}

#[test]
fn webm_needs_webm_codecs() {
    let info = map(probe(
        "matroska,webm",
        &[],
        vec![video("vp9"), audio("opus")],
    ));
    assert_eq!(info.container, ContainerKind::WebM);
    assert_eq!(info.mime_type.as_deref(), Some("video/webm"));
    let info = map(probe(
        "matroska,webm",
        &[],
        vec![video("h264"), audio("aac")],
    ));
    assert_eq!(info.container, ContainerKind::Matroska);
}

#[test]
fn pcm_name_gives_bits() {
    let info = map(probe(
        "wav",
        &[],
        vec![audio("pcm_s24le"), audio("pcm_f32le")],
    ));
    let codecs: Vec<_> = info
        .streams
        .iter()
        .map(|s| match s {
            Stream::Audio(a) => a.codec.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        codecs,
        vec![
            Codec::Pcm {
                bits: 24,
                float: false
            },
            Codec::Pcm {
                bits: 32,
                float: true
            }
        ]
    );
}

#[test]
fn attached_pic_becomes_a_picture() {
    let cover = RawStream {
        media_type: 0,
        codec_name: "png".into(),
        width: 3,
        height: 2,
        attached_pic: true,
        picture: Some(vec![1, 2, 3]),
        tags: vec![("comment".into(), "Cover (front)".into())],
        ..RawStream::default()
    };
    let info = map(RawProbe {
        id3v2: true,
        ..probe("mp3", &[], vec![audio("mp3"), cover])
    });
    assert_eq!(info.streams.len(), 1);
    assert_eq!(info.pictures.len(), 1);
    let picture = &info.pictures[0];
    assert_eq!(picture.mime, "image/png");
    assert_eq!((picture.width, picture.height), (Some(3), Some(2)));
    assert_eq!(picture.kind.as_deref(), Some("Cover (front)"));
    assert_eq!(picture.system, TagSystem::Id3v2);
}

#[test]
fn mp3_tag_system_follows_id3_presence() {
    let tags = [("title", "T")];
    let v2 = map(RawProbe {
        id3v2: true,
        id3v1: true,
        ..probe("mp3", &tags, vec![audio("mp3")])
    });
    assert_eq!(v2.tags[0].system, TagSystem::Id3v2);
    assert_eq!(v2.tags[0].key, TagKey::Title);
    let v1 = map(RawProbe {
        id3v1: true,
        ..probe("mp3", &tags, vec![audio("mp3")])
    });
    assert_eq!(v1.tags[0].system, TagSystem::Id3v1);
    let flac = map(probe("flac", &tags, vec![audio("flac")]));
    assert_eq!(flac.tags[0].system, TagSystem::Vorbis);
}

#[test]
fn creation_time_parses() {
    let info = map(probe(
        MOV,
        &[("creation_time", "2024-05-06T07:08:09.500000Z")],
        vec![video("h264")],
    ));
    let expected = UNIX_EPOCH + Duration::from_millis(1_714_979_289_500);
    assert_eq!(info.created_at, Some(expected));
    assert!(
        info.tags
            .iter()
            .all(|t| t.key != TagKey::Other("creation_time".into()))
    );
    let epoch = map(probe(
        MOV,
        &[("creation_time", "1970-01-01T00:00:00.000000Z")],
        vec![video("h264")],
    ));
    assert_eq!(epoch.created_at, None);
    let junk = map(probe(
        MOV,
        &[("creation_time", "yesterday")],
        vec![video("h264")],
    ));
    assert_eq!(junk.created_at, None);
}

#[test]
fn raw_tree_has_format_and_streams() {
    let info = map(probe("wav", &[("title", "T")], vec![audio("pcm_s16le")]));
    let (name, tree) = info.raw.reader.expect("the ffmpeg view");
    assert_eq!(name, "ffmpeg");
    assert_eq!(
        tree.get("format").and_then(|f| f.get("format_name")),
        Some(&Tree::Str("wav".into()))
    );
    let Some(Tree::List(streams)) = tree.get("streams") else {
        panic!("streams")
    };
    assert_eq!(
        streams[0].get("codec_name"),
        Some(&Tree::Str("pcm_s16le".into()))
    );
}
