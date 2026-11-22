//! The library-neutral description of a file as a PHP array: `$data['speedyspec']`, and what a PHP replacement
//! reader returns (`schema` 1).

use std::time::UNIX_EPOCH;

use speedyspec_wp_media_contracts::{
    BitrateMode, Codec, ContainerKind, ImageKind, MediaInfo, Stream, TagKey, TagSystem, Tree,
};

pub const SCHEMA: i64 = 1;

fn b(s: &str) -> Tree {
    Tree::Bytes(s.as_bytes().to_vec())
}

fn opt<T>(m: &mut Vec<(String, Tree)>, key: &str, value: Option<T>, f: impl Fn(T) -> Tree) {
    if let Some(v) = value {
        m.push((key.to_owned(), f(v)));
    }
}

pub(crate) fn container_name(c: &ContainerKind) -> String {
    match c {
        ContainerKind::Image(kind) => format!("image/{}", image_name(kind)),
        ContainerKind::Other(name) => format!("other/{name}"),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

pub(crate) fn image_name(kind: &ImageKind) -> String {
    match kind {
        ImageKind::Other(name) => name.clone(),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

pub(crate) fn codec_name(c: &Codec) -> String {
    match c {
        Codec::Pcm { bits, float } => format!("pcm_{}{bits}", if *float { "f" } else { "s" }),
        Codec::RealAudio(n) | Codec::Wmv(n) | Codec::RealVideo(n) | Codec::Other(n) => n.clone(),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

pub(crate) fn tag_key_name(k: &TagKey) -> String {
    match k {
        TagKey::Other(n) => n.clone(),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

pub(crate) fn system_name(s: TagSystem) -> String {
    format!("{s:?}").to_ascii_lowercase()
}

/// `info` as the neutral array, with the reader's raw view under its name (`ffmpeg`, `midi`).
pub fn neutral(info: &MediaInfo) -> Tree {
    let mut m: Vec<(String, Tree)> = vec![
        ("schema".into(), Tree::Int(SCHEMA)),
        ("container".into(), b(&container_name(&info.container))),
    ];
    opt(&mut m, "mime_type", info.mime_type.as_deref(), b);
    opt(&mut m, "duration", info.duration, |d| {
        Tree::Float(d.as_secs_f64())
    });
    opt(&mut m, "bitrate", info.bitrate, |v| Tree::Int(v as i64));
    opt(&mut m, "file_size", info.file_size, |v| Tree::Int(v as i64));
    opt(
        &mut m,
        "created_at",
        info.created_at
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok()),
        |d| Tree::Int(d.as_secs() as i64),
    );
    let streams = info
        .streams
        .iter()
        .map(|s| {
            let mut e: Vec<(String, Tree)> = Vec::new();
            match s {
                Stream::Audio(a) => {
                    e.push(("type".into(), b("audio")));
                    e.push(("codec".into(), b(&codec_name(&a.codec))));
                    opt(&mut e, "sample_rate", a.sample_rate, |v| {
                        Tree::Int(v.into())
                    });
                    opt(&mut e, "channels", a.channels, |v| Tree::Int(v.into()));
                    opt(&mut e, "channel_layout", a.channel_layout.as_deref(), b);
                    opt(&mut e, "bits_per_sample", a.bits_per_sample, |v| {
                        Tree::Int(v.into())
                    });
                    opt(&mut e, "lossless", a.lossless, Tree::Bool);
                    opt(&mut e, "bitrate", a.bitrate, |v| Tree::Int(v as i64));
                    opt(&mut e, "bitrate_mode", a.bitrate_mode, |m| {
                        b(if m == BitrateMode::Variable {
                            "vbr"
                        } else {
                            "cbr"
                        })
                    });
                    opt(&mut e, "encoder", a.encoder.as_deref(), b);
                }
                Stream::Video(v) => {
                    e.push(("type".into(), b("video")));
                    e.push(("codec".into(), b(&codec_name(&v.codec))));
                    opt(&mut e, "width", v.width, |x| Tree::Int(x.into()));
                    opt(&mut e, "height", v.height, |x| Tree::Int(x.into()));
                    opt(&mut e, "frame_rate", v.frame_rate, |(n, d)| {
                        Tree::List(vec![Tree::Int(n.into()), Tree::Int(d.into())])
                    });
                    opt(&mut e, "bitrate", v.bitrate, |x| Tree::Int(x as i64));
                    opt(&mut e, "lossless", v.lossless, Tree::Bool);
                    opt(&mut e, "encoder", v.encoder.as_deref(), b);
                }
                Stream::Image(i) => {
                    e.push(("type".into(), b("image")));
                    e.push(("kind".into(), b(&image_name(&i.kind))));
                    opt(&mut e, "width", i.width, |x| Tree::Int(x.into()));
                    opt(&mut e, "height", i.height, |x| Tree::Int(x.into()));
                    opt(&mut e, "bit_depth", i.bit_depth, |x| Tree::Int(x.into()));
                    opt(&mut e, "channels", i.channels, |x| Tree::Int(x.into()));
                    e.push(("animated".into(), Tree::Bool(i.animated)));
                    opt(&mut e, "lossless", i.lossless, Tree::Bool);
                }
                Stream::Data { codec } => {
                    e.push(("type".into(), b("data")));
                    e.push(("codec".into(), b(&codec_name(codec))));
                }
                _ => e.push(("type".into(), b("unknown"))),
            }
            Tree::Map(e)
        })
        .collect();
    m.push(("streams".into(), Tree::List(streams)));
    let tags = info
        .tags
        .iter()
        .map(|t| {
            Tree::Map(vec![
                ("key".into(), b(&tag_key_name(&t.key))),
                ("system".into(), b(&system_name(t.system))),
                (
                    "values".into(),
                    Tree::List(t.values.iter().map(|v| b(v)).collect()),
                ),
            ])
        })
        .collect();
    m.push(("tags".into(), Tree::List(tags)));
    let pictures = info
        .pictures
        .iter()
        .map(|p| {
            let mut e = vec![("mime".to_owned(), b(&p.mime))];
            opt(&mut e, "width", p.width, |x| Tree::Int(x.into()));
            opt(&mut e, "height", p.height, |x| Tree::Int(x.into()));
            opt(&mut e, "kind", p.kind.as_deref(), b);
            e.push(("system".into(), b(&system_name(p.system))));
            e.push(("data".into(), Tree::Bytes(p.data.clone())));
            Tree::Map(e)
        })
        .collect();
    m.push(("pictures".into(), Tree::List(pictures)));
    if let Some((name, tree)) = &info.raw.reader {
        m.push((name.clone(), tree.clone()));
    }
    Tree::Map(m)
}

const CONTAINERS: &[(&str, ContainerKind)] = &[
    ("mp4", ContainerKind::Mp4),
    ("quicktime", ContainerKind::QuickTime),
    ("threegp", ContainerKind::ThreeGp),
    ("matroska", ContainerKind::Matroska),
    ("webm", ContainerKind::WebM),
    ("ogg", ContainerKind::Ogg),
    ("asf", ContainerKind::Asf),
    ("wav", ContainerKind::Wav),
    ("avi", ContainerKind::Avi),
    ("aiff", ContainerKind::Aiff),
    ("flv", ContainerKind::Flv),
    ("mpegps", ContainerKind::MpegPs),
    ("mpegts", ContainerKind::MpegTs),
    ("mp3", ContainerKind::Mp3),
    ("aac", ContainerKind::Aac),
    ("flac", ContainerKind::Flac),
    ("ac3", ContainerKind::Ac3),
    ("dts", ContainerKind::Dts),
    ("realmedia", ContainerKind::RealMedia),
    ("midi", ContainerKind::Midi),
];

const IMAGES: &[(&str, ImageKind)] = &[
    ("jpeg", ImageKind::Jpeg),
    ("png", ImageKind::Png),
    ("gif", ImageKind::Gif),
    ("webp", ImageKind::Webp),
    ("bmp", ImageKind::Bmp),
    ("tiff", ImageKind::Tiff),
    ("avif", ImageKind::Avif),
    ("heif", ImageKind::Heif),
    ("jpegxl", ImageKind::JpegXl),
    ("ico", ImageKind::Ico),
];

const CODECS: &[(&str, Codec)] = &[
    ("mp3", Codec::Mp3),
    ("mp2", Codec::Mp2),
    ("aac", Codec::Aac),
    ("flac", Codec::Flac),
    ("vorbis", Codec::Vorbis),
    ("opus", Codec::Opus),
    ("alac", Codec::Alac),
    ("wmav1", Codec::WmaV1),
    ("wmav2", Codec::WmaV2),
    ("wmapro", Codec::WmaPro),
    ("wmalossless", Codec::WmaLossless),
    ("ac3", Codec::Ac3),
    ("eac3", Codec::Eac3),
    ("dts", Codec::Dts),
    ("truehd", Codec::TrueHd),
    ("wavpack", Codec::WavPack),
    ("ape", Codec::Ape),
    ("h264", Codec::H264),
    ("hevc", Codec::Hevc),
    ("av1", Codec::Av1),
    ("vp8", Codec::Vp8),
    ("vp9", Codec::Vp9),
    ("mpeg1video", Codec::Mpeg1Video),
    ("mpeg2video", Codec::Mpeg2Video),
    ("mpeg4part2", Codec::Mpeg4Part2),
    ("theora", Codec::Theora),
    ("jpeg", Codec::Jpeg),
    ("png", Codec::Png),
    ("gif", Codec::Gif),
    ("webp", Codec::Webp),
    ("bmp", Codec::Bmp),
    ("tiff", Codec::Tiff),
];

const KEYS: &[(&str, TagKey)] = &[
    ("title", TagKey::Title),
    ("artist", TagKey::Artist),
    ("album", TagKey::Album),
    ("albumartist", TagKey::AlbumArtist),
    ("year", TagKey::Year),
    ("genre", TagKey::Genre),
    ("track", TagKey::Track),
    ("disc", TagKey::Disc),
    ("comment", TagKey::Comment),
    ("composer", TagKey::Composer),
    ("copyright", TagKey::Copyright),
    ("encoder", TagKey::Encoder),
    ("lyrics", TagKey::Lyrics),
];

const SYSTEMS: &[(&str, TagSystem)] = &[
    ("id3v2", TagSystem::Id3v2),
    ("id3v1", TagSystem::Id3v1),
    ("vorbis", TagSystem::Vorbis),
    ("mp4", TagSystem::Mp4),
    ("asf", TagSystem::Asf),
    ("matroska", TagSystem::Matroska),
    ("riff", TagSystem::Riff),
    ("ape", TagSystem::Ape),
    ("midi", TagSystem::Midi),
    ("unknown", TagSystem::Unknown),
];

fn lookup<T: Clone>(table: &[(&str, T)], name: &str) -> Option<T> {
    table
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, v)| v.clone())
}

fn malformed(why: impl std::fmt::Display) -> speedyspec_wp_media_contracts::ReadError {
    speedyspec_wp_media_contracts::ReadError::Crashed(format!("malformed reader array: {why}"))
}

fn text(tree: Option<&Tree>) -> Option<String> {
    tree.and_then(Tree::bytes)
        .map(|b| String::from_utf8_lossy(b).into_owned())
}

fn number(tree: Option<&Tree>) -> Option<i64> {
    match tree? {
        Tree::Int(i) => Some(*i),
        Tree::Float(f) => Some(*f as i64),
        _ => None,
    }
}

fn unsigned<T: TryFrom<i64>>(tree: Option<&Tree>) -> Option<T> {
    number(tree).and_then(|n| T::try_from(n).ok())
}

fn flag(tree: Option<&Tree>) -> Option<bool> {
    match tree? {
        Tree::Bool(b) => Some(*b),
        _ => None,
    }
}

fn list(tree: Option<&Tree>) -> Vec<&Tree> {
    match tree {
        Some(Tree::List(l)) => l.iter().collect(),
        Some(Tree::Map(m)) => m.iter().map(|(_, v)| v).collect(),
        _ => Vec::new(),
    }
}

fn container_from(name: &str) -> ContainerKind {
    if let Some(kind) = name.strip_prefix("image/") {
        return ContainerKind::Image(
            lookup(IMAGES, kind).unwrap_or_else(|| ImageKind::Other(kind.into())),
        );
    }
    if let Some(other) = name.strip_prefix("other/") {
        return ContainerKind::Other(other.into());
    }
    lookup(CONTAINERS, name).unwrap_or_else(|| ContainerKind::Other(name.into()))
}

fn codec_from(name: &str) -> Codec {
    if let Some(rest) = name.strip_prefix("pcm_")
        && let Some(kind) = rest.chars().next()
        && let Some(Ok(bits)) = rest.get(kind.len_utf8()..).map(str::parse::<u16>)
        && (kind == 's' || kind == 'f')
    {
        return Codec::Pcm {
            bits,
            float: kind == 'f',
        };
    }
    lookup(CODECS, name).unwrap_or_else(|| Codec::Other(name.into()))
}

/// The neutral array back as a `MediaInfo` (what a PHP replacement reader returns). `Crashed` when it isn't the
/// schema-1 shape.
pub fn from_neutral(tree: &Tree) -> Result<MediaInfo, speedyspec_wp_media_contracts::ReadError> {
    use speedyspec_wp_media_contracts::{
        AudioStream, ImageStream, Picture, RawViews, Tag, VideoStream,
    };
    use std::time::Duration;

    if number(tree.get("schema")) != Some(SCHEMA) {
        return Err(malformed("schema must be 1"));
    }
    let container =
        container_from(&text(tree.get("container")).ok_or_else(|| malformed("no container"))?);
    let mut streams = Vec::new();
    for s in list(tree.get("streams")) {
        let codec = || codec_from(&text(s.get("codec")).unwrap_or_default());
        streams.push(match text(s.get("type")).as_deref() {
            Some("audio") => Stream::Audio(AudioStream {
                codec: codec(),
                sample_rate: unsigned(s.get("sample_rate")),
                channels: unsigned(s.get("channels")),
                channel_layout: text(s.get("channel_layout")),
                bits_per_sample: unsigned(s.get("bits_per_sample")),
                lossless: flag(s.get("lossless")),
                bitrate: unsigned(s.get("bitrate")),
                bitrate_mode: text(s.get("bitrate_mode")).map(|m| {
                    if m == "vbr" {
                        BitrateMode::Variable
                    } else {
                        BitrateMode::Constant
                    }
                }),
                encoder: text(s.get("encoder")),
            }),
            Some("video") => Stream::Video(VideoStream {
                codec: codec(),
                width: unsigned(s.get("width")),
                height: unsigned(s.get("height")),
                frame_rate: match list(s.get("frame_rate")).as_slice() {
                    [n, d] => unsigned(Some(n)).zip(unsigned(Some(d))),
                    _ => None,
                },
                bitrate: unsigned(s.get("bitrate")),
                lossless: flag(s.get("lossless")),
                encoder: text(s.get("encoder")),
            }),
            Some("image") => {
                let kind = text(s.get("kind")).unwrap_or_default();
                Stream::Image(ImageStream {
                    kind: lookup(IMAGES, &kind).unwrap_or(ImageKind::Other(kind)),
                    width: unsigned(s.get("width")),
                    height: unsigned(s.get("height")),
                    bit_depth: unsigned(s.get("bit_depth")),
                    channels: unsigned(s.get("channels")),
                    animated: flag(s.get("animated")).unwrap_or(false),
                    lossless: flag(s.get("lossless")),
                })
            }
            Some("data") => Stream::Data { codec: codec() },
            other => return Err(malformed(format!("stream type {other:?}"))),
        });
    }
    let tags = list(tree.get("tags"))
        .into_iter()
        .map(|t| {
            let key = text(t.get("key")).ok_or_else(|| malformed("a tag without a key"))?;
            Ok(Tag {
                key: lookup(KEYS, &key).unwrap_or(TagKey::Other(key)),
                system: lookup(SYSTEMS, &text(t.get("system")).unwrap_or_default())
                    .unwrap_or(TagSystem::Unknown),
                values: list(t.get("values"))
                    .into_iter()
                    .filter_map(|v| text(Some(v)))
                    .collect(),
            })
        })
        .collect::<Result<_, _>>()?;
    let pictures = list(tree.get("pictures"))
        .into_iter()
        .map(|p| {
            Ok(Picture {
                data: p
                    .get("data")
                    .and_then(Tree::bytes)
                    .ok_or_else(|| malformed("a picture without data"))?
                    .to_vec(),
                mime: text(p.get("mime")).unwrap_or_default(),
                width: unsigned(p.get("width")),
                height: unsigned(p.get("height")),
                kind: text(p.get("kind")),
                system: lookup(SYSTEMS, &text(p.get("system")).unwrap_or_default())
                    .unwrap_or(TagSystem::Unknown),
            })
        })
        .collect::<Result<_, _>>()?;
    let duration = match tree.get("duration") {
        Some(Tree::Float(f)) => Duration::try_from_secs_f64(*f).ok(),
        Some(Tree::Int(i)) if *i >= 0 => Some(Duration::from_secs(*i as u64)),
        _ => None,
    };
    let reader = ["ffmpeg", "midi"]
        .iter()
        .find_map(|name| tree.get(name).map(|t| (name.to_string(), t.clone())));
    Ok(MediaInfo {
        container,
        mime_type: text(tree.get("mime_type")),
        duration,
        bitrate: unsigned(tree.get("bitrate")),
        file_size: unsigned(tree.get("file_size")),
        created_at: unsigned::<u64>(tree.get("created_at"))
            .map(|s| UNIX_EPOCH + Duration::from_secs(s)),
        streams,
        tags,
        pictures,
        raw: RawViews {
            getid3: None,
            reader,
        },
    })
}
