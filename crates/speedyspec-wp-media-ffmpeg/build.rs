//! Compiles the C shim against the pinned FFmpeg's headers (`FFMPEG_PREFIX`); `speedyspec-ffmpeg-sys` links FFmpeg.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/shim.c");
    println!("cargo:rerun-if-changed=src/shim.h");
    println!("cargo:rerun-if-env-changed=FFMPEG_PREFIX");
    if env::var_os("DOCS_RS").is_some() {
        return;
    }
    let prefix = env::var_os("FFMPEG_PREFIX").map(PathBuf::from).unwrap_or_else(|| {
        eprintln!("ffmpeg: FFMPEG_PREFIX is not set; point it at the prefix scripts/build-ffmpeg.sh builds");
        std::process::exit(1);
    });
    cc::Build::new()
        .file("src/shim.c")
        .include(prefix.join("include"))
        .std("c11")
        .opt_level(2)
        .warnings(true)
        .extra_warnings(true)
        .warnings_into_errors(true)
        .compile("speedyspec_media_shim");
}
