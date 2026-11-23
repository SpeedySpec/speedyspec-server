//! The media reader port and the library-neutral description of a media file.
//!
//! WordPress reads audio, video and image facts through getID3, `getimagesize()` and hand-written parsers. SpeedySpec
//! reads them through one port, [`MediaReader`], whose answer, [`MediaInfo`], describes a file without reference to
//! any library. Readers (FFmpeg, MIDI, a PHP plugin's replacement) are adapters at the edge; WordPress's rules map
//! `MediaInfo` to WordPress's arrays. Each reader may attach its own raw view ([`RawViews`]) for callers that need
//! more than the neutral fields.
//!
//! Everything here is plain data: no I/O, no serialization, no library types.

mod info;
mod ports;
mod reader;
mod tree;

pub use info::{
    AudioStream, BitrateMode, Codec, ContainerKind, ImageKind, ImageStream, MediaInfo, Picture,
    RawViews, Stream, Tag, TagKey, TagSystem, VideoStream,
};
pub use ports::{ContentSniffer, TagSanitizer, TimeParser};
pub use reader::{Limit, MEDIA_READER, MediaReader, MediaSource, ProbeLimits, ReadError};
pub use tree::Tree;
