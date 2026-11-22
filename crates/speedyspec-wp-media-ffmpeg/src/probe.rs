//! What the shim found, copied into owned Rust values so the Data Mapper is tested without FFmpeg.

use std::ffi::{CStr, c_char};

use crate::ffi::{SsResult, SsStream, SsTag};

/// One stream as FFmpeg described it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawStream {
    /// 0 video, 1 audio, 2 data, 3 subtitle, 4 attachment, -1 unknown.
    pub media_type: i32,
    pub codec_name: String,
    pub profile: Option<String>,
    pub width: u32,
    pub height: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub channel_layout: Option<String>,
    pub bits_per_raw_sample: u16,
    pub bits_per_coded_sample: u16,
    pub bit_rate: u64,
    pub frame_rate: Option<(u32, u32)>,
    pub pix_fmt: Option<String>,
    pub sample_fmt: Option<String>,
    pub attached_pic: bool,
    pub picture: Option<Vec<u8>>,
    pub tags: Vec<(String, String)>,
}

/// A whole file as FFmpeg described it, plus what the reader checked itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawProbe {
    pub format_name: String,
    pub duration_us: Option<i64>,
    pub bit_rate: Option<i64>,
    pub file_size: u64,
    pub tags: Vec<(String, String)>,
    pub streams: Vec<RawStream>,
    /// The file starts with an ID3v2 header (`ID3`).
    pub id3v2: bool,
    /// The file ends with an ID3v1 tag (`TAG` 128 bytes from the end).
    pub id3v1: bool,
}

fn string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the shim stores NUL-terminated strings it allocated, alive until ss_free.
    Some(
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned(),
    )
}

fn slice<'a, T>(ptr: *const T, len: i32) -> &'a [T] {
    if ptr.is_null() || len <= 0 {
        return &[];
    }
    // SAFETY: the shim allocated `len` contiguous elements at `ptr`, alive until ss_free.
    unsafe { std::slice::from_raw_parts(ptr, len as usize) }
}

fn tags(ptr: *const SsTag, len: i32) -> Vec<(String, String)> {
    slice(ptr, len)
        .iter()
        .filter_map(|tag| Some((string(tag.key)?, string(tag.value)?)))
        .collect()
}

fn positive_u32(v: i32) -> u32 {
    u32::try_from(v).unwrap_or(0)
}

fn stream(s: &SsStream) -> RawStream {
    RawStream {
        media_type: s.media_type,
        codec_name: string(s.codec_name).unwrap_or_default(),
        profile: string(s.profile),
        width: positive_u32(s.width),
        height: positive_u32(s.height),
        sample_rate: positive_u32(s.sample_rate),
        channels: u16::try_from(s.channels).unwrap_or(0),
        channel_layout: string(s.channel_layout),
        bits_per_raw_sample: u16::try_from(s.bits_per_raw_sample).unwrap_or(0),
        bits_per_coded_sample: u16::try_from(s.bits_per_coded_sample).unwrap_or(0),
        bit_rate: u64::try_from(s.bit_rate).unwrap_or(0),
        frame_rate: (s.fr_num > 0 && s.fr_den > 0).then_some((s.fr_num as u32, s.fr_den as u32)),
        pix_fmt: string(s.pix_fmt),
        sample_fmt: string(s.sample_fmt),
        attached_pic: s.attached_pic != 0,
        picture: (!s.picture.is_null() && s.picture_size > 0)
            .then(|| slice(s.picture, s.picture_size as i32).to_vec()),
        tags: tags(s.tags, s.tag_count),
    }
}

/// Copies the shim's result. `file_size`, `id3v2` and `id3v1` are filled by the caller.
pub(crate) fn copy(result: &SsResult) -> RawProbe {
    RawProbe {
        format_name: string(result.format_name).unwrap_or_default(),
        duration_us: (result.duration_us != i64::MIN && result.duration_us >= 0)
            .then_some(result.duration_us),
        bit_rate: (result.bit_rate > 0).then_some(result.bit_rate),
        file_size: 0,
        tags: tags(result.tags, result.tag_count),
        streams: slice(result.streams, result.stream_count)
            .iter()
            .map(stream)
            .collect(),
        id3v2: false,
        id3v1: false,
    }
}
