//! The media reader over FFmpeg (Adapter, Data Mapper).
//!
//! A C shim (`shim.c`) reads the file through its own I/O context, within byte, time and stream limits, with no
//! protocols and nested opens refused, and copies what libavformat found into plain structs. [`FfmpegReader`] copies
//! those into a [`RawProbe`] and the mapper turns it into a library-neutral `MediaInfo`, keeping FFmpeg's own view as
//! the raw view.

// Links the pinned FFmpeg the shim calls.
extern crate speedyspec_ffmpeg_sys;

mod ffi;
mod mapper;
mod probe;
mod reader;

pub use mapper::map;
pub use probe::{RawProbe, RawStream};
pub use reader::FfmpegReader;
