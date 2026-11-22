//! The pinned, LGPL-only static FFmpeg (`scripts/build-ffmpeg.sh`), linked once for the runtime.
//!
//! The build script links `libavformat`, `libavcodec` and `libavutil` from `FFMPEG_PREFIX` and zlib from the static
//! PHP's own `libz.a`. This crate declares only what identifies the build: its version, licence, configuration and
//! which demuxers and protocols it has. The media adapter reads files through its own C shim compiled against the
//! same headers, so no FFmpeg struct layout is read from Rust.

use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};

unsafe extern "C" {
    pub fn avformat_version() -> c_uint;
    pub fn avformat_license() -> *const c_char;
    pub fn avformat_configuration() -> *const c_char;
    pub fn av_version_info() -> *const c_char;
    pub fn av_find_input_format(short_name: *const c_char) -> *const c_void;
    pub fn avio_enum_protocols(opaque: *mut *mut c_void, output: c_int) -> *const c_char;
}

fn static_str(ptr: *const c_char) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    // SAFETY: FFmpeg returns pointers to static, NUL-terminated ASCII strings that live for the whole process.
    unsafe { CStr::from_ptr(ptr) }.to_str().unwrap_or("")
}

/// The FFmpeg release the runtime was built with, as FFmpeg reports it (for example `9.1`).
pub fn version() -> &'static str {
    // SAFETY: no arguments; returns a static string.
    static_str(unsafe { av_version_info() })
}

/// FFmpeg's own statement of the licence its configuration puts it under.
pub fn licence() -> &'static str {
    // SAFETY: no arguments; returns a static string.
    static_str(unsafe { avformat_license() })
}

/// The `configure` arguments libavformat was built with.
pub fn configuration() -> &'static str {
    // SAFETY: no arguments; returns a static string.
    static_str(unsafe { avformat_configuration() })
}

/// Whether the build can read the container format FFmpeg calls `name`.
pub fn has_demuxer(name: &str) -> bool {
    let Ok(name) = CString::new(name) else {
        return false;
    };
    // SAFETY: `name` is a valid NUL-terminated string for the duration of the call; the result is only compared.
    !unsafe { av_find_input_format(name.as_ptr()) }.is_null()
}

/// The input protocols built in: empty, because the runtime hands FFmpeg its own reader.
pub fn input_protocols() -> Vec<&'static str> {
    let mut opaque: *mut c_void = std::ptr::null_mut();
    let mut names = Vec::new();
    loop {
        // SAFETY: `opaque` starts null and is only advanced by FFmpeg, as avio_enum_protocols documents.
        let name = unsafe { avio_enum_protocols(&mut opaque, 0) };
        if name.is_null() {
            return names;
        }
        names.push(static_str(name));
    }
}
