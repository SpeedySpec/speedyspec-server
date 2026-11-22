//! The `extern "C"` mirror of `shim.h`. Field order and types match the header exactly.

use std::ffi::{c_char, c_int};

pub const SS_OK: i32 = 0;
pub const SS_UNRECOGNIZED: i32 = 1;
pub const SS_TRUNCATED: i32 = 2;
pub const SS_LIMIT_BYTES: i32 = 3;
pub const SS_LIMIT_STREAMS: i32 = 4;
pub const SS_TIMEOUT: i32 = 5;
pub const SS_IO: i32 = 6;
pub const SS_NOMEM: i32 = 7;

#[repr(C)]
pub struct SsLimits {
    pub max_bytes: i64,
    pub deadline_ms: i64,
    pub max_streams: i32,
    pub max_picture_bytes: i64,
    pub max_tag_bytes: i64,
}

#[repr(C)]
pub struct SsTag {
    pub key: *mut c_char,
    pub value: *mut c_char,
}

#[repr(C)]
pub struct SsStream {
    pub media_type: i32,
    pub codec_name: *mut c_char,
    pub profile: *mut c_char,
    pub width: i32,
    pub height: i32,
    pub sample_rate: i32,
    pub channels: i32,
    pub channel_layout: *mut c_char,
    pub bits_per_raw_sample: i32,
    pub bits_per_coded_sample: i32,
    pub bit_rate: i64,
    pub fr_num: i32,
    pub fr_den: i32,
    pub pix_fmt: *mut c_char,
    pub sample_fmt: *mut c_char,
    pub attached_pic: i32,
    pub picture: *mut u8,
    pub picture_size: i64,
    pub tags: *mut SsTag,
    pub tag_count: i32,
}

#[repr(C)]
pub struct SsResult {
    pub format_name: *mut c_char,
    pub duration_us: i64,
    pub bit_rate: i64,
    pub bytes_read: i64,
    pub tags: *mut SsTag,
    pub tag_count: i32,
    pub streams: *mut SsStream,
    pub stream_count: i32,
}

unsafe extern "C" {
    pub fn ss_global_init(max_alloc: i64);
    pub fn ss_probe(fd: c_int, limits: *const SsLimits, out: *mut *mut SsResult) -> i32;
    pub fn ss_free(result: *mut SsResult);
}
