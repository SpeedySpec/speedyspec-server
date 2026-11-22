//! `MediaInfo`: what a media file is, in terms of no particular library.

use std::time::{Duration, SystemTime};

use crate::Tree;

/// A media file's description. Fields a reader can't determine are `None` or empty.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaInfo {
    pub container: ContainerKind,
    pub mime_type: Option<String>,
    pub duration: Option<Duration>,
    pub bitrate: Option<u64>,
    pub file_size: Option<u64>,
    pub created_at: Option<SystemTime>,
    pub streams: Vec<Stream>,
    pub tags: Vec<Tag>,
    pub pictures: Vec<Picture>,
    pub raw: RawViews,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContainerKind {
    Mp4,
    QuickTime,
    ThreeGp,
    Matroska,
    WebM,
    Ogg,
    Asf,
    Wav,
    Avi,
    Aiff,
    Flv,
    MpegPs,
    MpegTs,
    Mp3,
    Aac,
    Flac,
    Ac3,
    Dts,
    RealMedia,
    Midi,
    Image(ImageKind),
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageKind {
    Jpeg,
    Png,
    Gif,
    Webp,
    Bmp,
    Tiff,
    Avif,
    Heif,
    JpegXl,
    Ico,
    Other(String),
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Stream {
    Audio(AudioStream),
    Video(VideoStream),
    Image(ImageStream),
    Data { codec: Codec },
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioStream {
    pub codec: Codec,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub channel_layout: Option<String>,
    pub bits_per_sample: Option<u16>,
    pub lossless: Option<bool>,
    pub bitrate: Option<u64>,
    pub bitrate_mode: Option<BitrateMode>,
    pub encoder: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VideoStream {
    pub codec: Codec,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Frames per second as a fraction (numerator, denominator).
    pub frame_rate: Option<(u32, u32)>,
    pub bitrate: Option<u64>,
    pub lossless: Option<bool>,
    pub encoder: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageStream {
    pub kind: ImageKind,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
    pub animated: bool,
    pub lossless: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitrateMode {
    Constant,
    Variable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Codec {
    Mp3,
    Mp2,
    Aac,
    Flac,
    Vorbis,
    Opus,
    Pcm { bits: u16, float: bool },
    Alac,
    WmaV1,
    WmaV2,
    WmaPro,
    WmaLossless,
    Ac3,
    Eac3,
    Dts,
    TrueHd,
    RealAudio(String),
    WavPack,
    Ape,
    H264,
    Hevc,
    Av1,
    Vp8,
    Vp9,
    Mpeg1Video,
    Mpeg2Video,
    Mpeg4Part2,
    Wmv(String),
    Theora,
    RealVideo(String),
    Jpeg,
    Png,
    Gif,
    Webp,
    Bmp,
    Tiff,
    Other(String),
}

/// One tag, with every value it holds, and the tag format it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub key: TagKey,
    pub system: TagSystem,
    pub values: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TagKey {
    Title,
    Artist,
    Album,
    AlbumArtist,
    Year,
    Genre,
    Track,
    Disc,
    Comment,
    Composer,
    Copyright,
    Encoder,
    Lyrics,
    Other(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TagSystem {
    Id3v2,
    Id3v1,
    Vorbis,
    Mp4,
    Asf,
    Matroska,
    Riff,
    Ape,
    Midi,
    Unknown,
}

/// An embedded picture (cover art).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub data: Vec<u8>,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// The picture's role as the file names it (for example "Cover (front)").
    pub kind: Option<String>,
    pub system: TagSystem,
}

/// The raw views a `MediaInfo` carries: the getID3-shaped tree (filled by WordPress's rules) and the reader's own,
/// named (`"ffmpeg"`, `"midi"`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawViews {
    pub getid3: Option<Tree>,
    pub reader: Option<(String, Tree)>,
}
