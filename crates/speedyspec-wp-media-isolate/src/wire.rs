//! How a `MediaInfo` (or a `ReadError`) travels from the probe child to its parent, and into the result cache.
//!
//! A two-byte little-endian format version, then `postcard` bytes of DTOs mirroring every contracts type (Data
//! Mapper; the contracts crate stays free of serialization). Decoding rejects any other version, so a parent never
//! misreads a child or cache entry from another release.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use speedyspec_wp_media_contracts as c;

const VERSION: u16 = 1;

/// Why bytes could not be decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireError {
    Version(u16),
    Malformed(String),
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::Version(v) => write!(f, "wire format version {v}, expected {VERSION}"),
            WireError::Malformed(why) => write!(f, "malformed wire bytes: {why}"),
        }
    }
}

impl std::error::Error for WireError {}

#[derive(Serialize, Deserialize)]
enum ResultDto {
    Ok(InfoDto),
    Err(ErrorDto),
}

#[derive(Serialize, Deserialize)]
enum ErrorDto {
    NotFound,
    Unrecognized,
    Truncated,
    Limit(u8),
    TimedOut,
    Io(String),
    Crashed(String),
}

#[derive(Serialize, Deserialize)]
struct InfoDto {
    container: ContainerDto,
    mime_type: Option<String>,
    duration: Option<(u64, u32)>,
    bitrate: Option<u64>,
    file_size: Option<u64>,
    created_at: Option<(u64, u32)>,
    streams: Vec<StreamDto>,
    tags: Vec<TagDto>,
    pictures: Vec<PictureDto>,
    getid3: Option<TreeDto>,
    reader: Option<(String, TreeDto)>,
}

#[derive(Serialize, Deserialize)]
enum ContainerDto {
    Named(String),
    Image(ImageDto),
    Other(String),
}

#[derive(Serialize, Deserialize)]
enum ImageDto {
    Named(String),
    Other(String),
}

#[derive(Serialize, Deserialize)]
enum StreamDto {
    Audio {
        codec: CodecDto,
        sample_rate: Option<u32>,
        channels: Option<u16>,
        channel_layout: Option<String>,
        bits_per_sample: Option<u16>,
        lossless: Option<bool>,
        bitrate: Option<u64>,
        variable_bitrate: Option<bool>,
        encoder: Option<String>,
    },
    Video {
        codec: CodecDto,
        width: Option<u32>,
        height: Option<u32>,
        frame_rate: Option<(u32, u32)>,
        bitrate: Option<u64>,
        lossless: Option<bool>,
        encoder: Option<String>,
    },
    Image {
        kind: ImageDto,
        width: Option<u32>,
        height: Option<u32>,
        bit_depth: Option<u8>,
        channels: Option<u8>,
        animated: bool,
        lossless: Option<bool>,
    },
    Data {
        codec: CodecDto,
    },
}

#[derive(Serialize, Deserialize)]
enum CodecDto {
    Named(String),
    Pcm { bits: u16, float: bool },
    WithName(String, String),
    Other(String),
}

#[derive(Serialize, Deserialize)]
struct TagDto {
    key: (String, Option<String>),
    system: String,
    values: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct PictureDto {
    data: Vec<u8>,
    mime: String,
    width: Option<u32>,
    height: Option<u32>,
    kind: Option<String>,
    system: String,
}

#[derive(Serialize, Deserialize)]
enum TreeDto {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    List(Vec<TreeDto>),
    Map(Vec<(String, TreeDto)>),
}

macro_rules! named {
    ($to:ident, $from:ident, $ty:ty, [$($variant:ident),* $(,)?]) => {
        fn $to(v: &$ty) -> Option<&'static str> {
            match v {
                $(<$ty>::$variant => Some(stringify!($variant)),)*
                #[allow(unreachable_patterns)]
                _ => None,
            }
        }
        fn $from(name: &str) -> Option<$ty> {
            match name {
                $(stringify!($variant) => Some(<$ty>::$variant),)*
                _ => None,
            }
        }
    };
}

named!(container_name, container_from, c::ContainerKind, [
    Mp4, QuickTime, ThreeGp, Matroska, WebM, Ogg, Asf, Wav, Avi, Aiff, Flv, MpegPs, MpegTs, Mp3, Aac, Flac, Ac3, Dts,
    RealMedia, Midi,
]);
named!(image_name, image_from, c::ImageKind, [Jpeg, Png, Gif, Webp, Bmp, Tiff, Avif, Heif, JpegXl, Ico]);
named!(codec_name, codec_from, c::Codec, [
    Mp3, Mp2, Aac, Flac, Vorbis, Opus, Alac, WmaV1, WmaV2, WmaPro, WmaLossless, Ac3, Eac3, Dts, TrueHd, WavPack, Ape,
    H264, Hevc, Av1, Vp8, Vp9, Mpeg1Video, Mpeg2Video, Mpeg4Part2, Theora, Jpeg, Png, Gif, Webp, Bmp, Tiff,
]);
named!(key_name, key_from, c::TagKey, [
    Title, Artist, Album, AlbumArtist, Year, Genre, Track, Disc, Comment, Composer, Copyright, Encoder, Lyrics,
]);
named!(system_name, system_from, c::TagSystem, [Id3v2, Id3v1, Vorbis, Mp4, Asf, Matroska, Riff, Ape, Midi, Unknown]);

fn bad(what: &str) -> WireError {
    WireError::Malformed(format!("unknown {what}"))
}

fn image_dto(kind: &c::ImageKind) -> ImageDto {
    match (image_name(kind), kind) {
        (Some(name), _) => ImageDto::Named(name.into()),
        (None, c::ImageKind::Other(other)) => ImageDto::Other(other.clone()),
        (None, other) => ImageDto::Other(format!("{other:?}")),
    }
}

fn image_back(dto: ImageDto) -> Result<c::ImageKind, WireError> {
    match dto {
        ImageDto::Named(name) => image_from(&name).ok_or_else(|| bad("image kind")),
        ImageDto::Other(other) => Ok(c::ImageKind::Other(other)),
    }
}

fn codec_dto(codec: &c::Codec) -> CodecDto {
    if let Some(name) = codec_name(codec) {
        return CodecDto::Named(name.into());
    }
    match codec {
        c::Codec::Pcm { bits, float } => CodecDto::Pcm { bits: *bits, float: *float },
        c::Codec::RealAudio(n) => CodecDto::WithName("RealAudio".into(), n.clone()),
        c::Codec::Wmv(n) => CodecDto::WithName("Wmv".into(), n.clone()),
        c::Codec::RealVideo(n) => CodecDto::WithName("RealVideo".into(), n.clone()),
        c::Codec::Other(n) => CodecDto::Other(n.clone()),
        other => CodecDto::Other(format!("{other:?}")),
    }
}

fn codec_back(dto: CodecDto) -> Result<c::Codec, WireError> {
    Ok(match dto {
        CodecDto::Named(name) => codec_from(&name).ok_or_else(|| bad("codec"))?,
        CodecDto::Pcm { bits, float } => c::Codec::Pcm { bits, float },
        CodecDto::WithName(kind, n) => match kind.as_str() {
            "RealAudio" => c::Codec::RealAudio(n),
            "Wmv" => c::Codec::Wmv(n),
            "RealVideo" => c::Codec::RealVideo(n),
            _ => return Err(bad("codec family")),
        },
        CodecDto::Other(n) => c::Codec::Other(n),
    })
}

fn system_dto(system: c::TagSystem) -> String {
    system_name(&system).unwrap_or("Unknown").to_owned()
}

fn system_back(name: &str) -> Result<c::TagSystem, WireError> {
    system_from(name).ok_or_else(|| bad("tag system"))
}

fn tree_dto(tree: &c::Tree) -> TreeDto {
    match tree {
        c::Tree::Null => TreeDto::Null,
        c::Tree::Bool(b) => TreeDto::Bool(*b),
        c::Tree::Int(i) => TreeDto::Int(*i),
        c::Tree::Float(f) => TreeDto::Float(*f),
        c::Tree::Str(s) => TreeDto::Str(s.clone()),
        c::Tree::Bytes(b) => TreeDto::Bytes(b.clone()),
        c::Tree::List(l) => TreeDto::List(l.iter().map(tree_dto).collect()),
        c::Tree::Map(m) => TreeDto::Map(m.iter().map(|(k, v)| (k.clone(), tree_dto(v))).collect()),
    }
}

fn tree_back(dto: TreeDto) -> c::Tree {
    match dto {
        TreeDto::Null => c::Tree::Null,
        TreeDto::Bool(b) => c::Tree::Bool(b),
        TreeDto::Int(i) => c::Tree::Int(i),
        TreeDto::Float(f) => c::Tree::Float(f),
        TreeDto::Str(s) => c::Tree::Str(s),
        TreeDto::Bytes(b) => c::Tree::Bytes(b),
        TreeDto::List(l) => c::Tree::List(l.into_iter().map(tree_back).collect()),
        TreeDto::Map(m) => c::Tree::Map(m.into_iter().map(|(k, v)| (k, tree_back(v))).collect()),
    }
}

fn duration_dto(d: Duration) -> (u64, u32) {
    (d.as_secs(), d.subsec_nanos())
}

fn time_dto(t: SystemTime) -> Option<(u64, u32)> {
    t.duration_since(UNIX_EPOCH).ok().map(duration_dto)
}

fn info_dto(info: &c::MediaInfo) -> InfoDto {
    let container = match (container_name(&info.container), &info.container) {
        (Some(name), _) => ContainerDto::Named(name.into()),
        (None, c::ContainerKind::Image(kind)) => ContainerDto::Image(image_dto(kind)),
        (None, c::ContainerKind::Other(other)) => ContainerDto::Other(other.clone()),
        (None, other) => ContainerDto::Other(format!("{other:?}")),
    };
    let streams = info
        .streams
        .iter()
        .map(|s| match s {
            c::Stream::Audio(a) => StreamDto::Audio {
                codec: codec_dto(&a.codec),
                sample_rate: a.sample_rate,
                channels: a.channels,
                channel_layout: a.channel_layout.clone(),
                bits_per_sample: a.bits_per_sample,
                lossless: a.lossless,
                bitrate: a.bitrate,
                variable_bitrate: a.bitrate_mode.map(|m| m == c::BitrateMode::Variable),
                encoder: a.encoder.clone(),
            },
            c::Stream::Video(v) => StreamDto::Video {
                codec: codec_dto(&v.codec),
                width: v.width,
                height: v.height,
                frame_rate: v.frame_rate,
                bitrate: v.bitrate,
                lossless: v.lossless,
                encoder: v.encoder.clone(),
            },
            c::Stream::Image(i) => StreamDto::Image {
                kind: image_dto(&i.kind),
                width: i.width,
                height: i.height,
                bit_depth: i.bit_depth,
                channels: i.channels,
                animated: i.animated,
                lossless: i.lossless,
            },
            c::Stream::Data { codec } => StreamDto::Data { codec: codec_dto(codec) },
            other => StreamDto::Data { codec: CodecDto::Other(format!("{other:?}")) },
        })
        .collect();
    InfoDto {
        container,
        mime_type: info.mime_type.clone(),
        duration: info.duration.map(duration_dto),
        bitrate: info.bitrate,
        file_size: info.file_size,
        created_at: info.created_at.and_then(time_dto),
        streams,
        tags: info
            .tags
            .iter()
            .map(|t| TagDto {
                key: match (key_name(&t.key), &t.key) {
                    (Some(name), _) => (name.into(), None),
                    (None, c::TagKey::Other(o)) => ("Other".into(), Some(o.clone())),
                    (None, other) => ("Other".into(), Some(format!("{other:?}"))),
                },
                system: system_dto(t.system),
                values: t.values.clone(),
            })
            .collect(),
        pictures: info
            .pictures
            .iter()
            .map(|p| PictureDto {
                data: p.data.clone(),
                mime: p.mime.clone(),
                width: p.width,
                height: p.height,
                kind: p.kind.clone(),
                system: system_dto(p.system),
            })
            .collect(),
        getid3: info.raw.getid3.as_ref().map(tree_dto),
        reader: info.raw.reader.as_ref().map(|(n, t)| (n.clone(), tree_dto(t))),
    }
}

fn info_back(dto: InfoDto) -> Result<c::MediaInfo, WireError> {
    let container = match dto.container {
        ContainerDto::Named(name) => container_from(&name).ok_or_else(|| bad("container"))?,
        ContainerDto::Image(kind) => c::ContainerKind::Image(image_back(kind)?),
        ContainerDto::Other(other) => c::ContainerKind::Other(other),
    };
    let streams = dto
        .streams
        .into_iter()
        .map(|s| {
            Ok(match s {
                StreamDto::Audio {
                    codec,
                    sample_rate,
                    channels,
                    channel_layout,
                    bits_per_sample,
                    lossless,
                    bitrate,
                    variable_bitrate,
                    encoder,
                } => c::Stream::Audio(c::AudioStream {
                    codec: codec_back(codec)?,
                    sample_rate,
                    channels,
                    channel_layout,
                    bits_per_sample,
                    lossless,
                    bitrate,
                    bitrate_mode: variable_bitrate
                        .map(|v| if v { c::BitrateMode::Variable } else { c::BitrateMode::Constant }),
                    encoder,
                }),
                StreamDto::Video { codec, width, height, frame_rate, bitrate, lossless, encoder } => {
                    c::Stream::Video(c::VideoStream {
                        codec: codec_back(codec)?,
                        width,
                        height,
                        frame_rate,
                        bitrate,
                        lossless,
                        encoder,
                    })
                }
                StreamDto::Image { kind, width, height, bit_depth, channels, animated, lossless } => {
                    c::Stream::Image(c::ImageStream {
                        kind: image_back(kind)?,
                        width,
                        height,
                        bit_depth,
                        channels,
                        animated,
                        lossless,
                    })
                }
                StreamDto::Data { codec } => c::Stream::Data { codec: codec_back(codec)? },
            })
        })
        .collect::<Result<_, WireError>>()?;
    let tags = dto
        .tags
        .into_iter()
        .map(|t| {
            let key = match t.key {
                (name, None) => key_from(&name).ok_or_else(|| bad("tag key"))?,
                (_, Some(other)) => c::TagKey::Other(other),
            };
            Ok(c::Tag { key, system: system_back(&t.system)?, values: t.values })
        })
        .collect::<Result<_, WireError>>()?;
    let pictures = dto
        .pictures
        .into_iter()
        .map(|p| {
            Ok(c::Picture {
                data: p.data,
                mime: p.mime,
                width: p.width,
                height: p.height,
                kind: p.kind,
                system: system_back(&p.system)?,
            })
        })
        .collect::<Result<_, WireError>>()?;
    Ok(c::MediaInfo {
        container,
        mime_type: dto.mime_type,
        duration: dto.duration.map(|(s, n)| Duration::new(s, n)),
        bitrate: dto.bitrate,
        file_size: dto.file_size,
        created_at: dto.created_at.map(|(s, n)| UNIX_EPOCH + Duration::new(s, n)),
        streams,
        tags,
        pictures,
        raw: c::RawViews { getid3: dto.getid3.map(tree_back), reader: dto.reader.map(|(n, t)| (n, tree_back(t))) },
    })
}

fn limit_code(limit: c::Limit) -> u8 {
    match limit {
        c::Limit::Bytes => 0,
        c::Limit::Streams => 1,
        c::Limit::Memory => 2,
        c::Limit::Cpu => 3,
        _ => 4,
    }
}

fn limit_back(code: u8) -> Result<c::Limit, WireError> {
    Ok(match code {
        0 => c::Limit::Bytes,
        1 => c::Limit::Streams,
        2 => c::Limit::Memory,
        3 => c::Limit::Cpu,
        4 => c::Limit::Output,
        _ => return Err(bad("limit")),
    })
}

/// The bytes of a read's outcome.
pub fn encode(result: &Result<c::MediaInfo, c::ReadError>) -> Vec<u8> {
    let dto = match result {
        Ok(info) => ResultDto::Ok(info_dto(info)),
        Err(err) => ResultDto::Err(match err {
            c::ReadError::NotFound => ErrorDto::NotFound,
            c::ReadError::Unrecognized => ErrorDto::Unrecognized,
            c::ReadError::Truncated => ErrorDto::Truncated,
            c::ReadError::LimitExceeded(limit) => ErrorDto::Limit(limit_code(*limit)),
            c::ReadError::TimedOut => ErrorDto::TimedOut,
            c::ReadError::Io(what) => ErrorDto::Io(what.clone()),
            c::ReadError::Crashed(what) => ErrorDto::Crashed(what.clone()),
            other => ErrorDto::Crashed(other.to_string()),
        }),
    };
    let mut out = VERSION.to_le_bytes().to_vec();
    out.extend(postcard::to_stdvec(&dto).expect("wire DTOs always serialize"));
    out
}

/// A read's outcome from its bytes.
pub fn decode(bytes: &[u8]) -> Result<Result<c::MediaInfo, c::ReadError>, WireError> {
    if bytes.len() < 2 {
        return Err(WireError::Malformed("shorter than its version".into()));
    }
    let version = u16::from_le_bytes([bytes[0], bytes[1]]);
    if version != VERSION {
        return Err(WireError::Version(version));
    }
    let dto: ResultDto = postcard::from_bytes(&bytes[2..]).map_err(|e| WireError::Malformed(e.to_string()))?;
    Ok(match dto {
        ResultDto::Ok(info) => Ok(info_back(info)?),
        ResultDto::Err(err) => Err(match err {
            ErrorDto::NotFound => c::ReadError::NotFound,
            ErrorDto::Unrecognized => c::ReadError::Unrecognized,
            ErrorDto::Truncated => c::ReadError::Truncated,
            ErrorDto::Limit(code) => c::ReadError::LimitExceeded(limit_back(code)?),
            ErrorDto::TimedOut => c::ReadError::TimedOut,
            ErrorDto::Io(what) => c::ReadError::Io(what),
            ErrorDto::Crashed(what) => c::ReadError::Crashed(what),
        }),
    })
}
