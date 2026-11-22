//! The linked FFmpeg is the pinned, LGPL-only build: checked against the library itself, not the build script's lists.

#[test]
fn licence_is_lgpl_2_1_or_later() {
    assert_eq!(
        speedyspec_ffmpeg_sys::licence(),
        "LGPL version 2.1 or later"
    );
    let conf = speedyspec_ffmpeg_sys::configuration();
    for forbidden in ["--enable-gpl", "--enable-version3", "--enable-nonfree"] {
        assert!(!conf.contains(forbidden), "{forbidden} in {conf}");
    }
    assert!(conf.contains("--enable-small"), "{conf}");
}

/// Names are FFmpeg's run-time demuxer names (`png_pipe`), not configure's component names (`image_png_pipe`).
#[test]
fn reads_the_formats_wordpress_accepts() {
    for name in [
        "mp3",
        "aac",
        "mov",
        "matroska",
        "ogg",
        "flac",
        "wav",
        "aiff",
        "asf",
        "rm",
        "flv",
        "avi",
        "mpeg",
        "mpegts",
        "ac3",
        "dts",
        "png_pipe",
        "jpeg_pipe",
        "webp_pipe",
        "gif_pipe",
        "bmp_pipe",
        "tiff_pipe",
    ] {
        assert!(
            speedyspec_ffmpeg_sys::has_demuxer(name),
            "missing demuxer {name}"
        );
    }
}

#[test]
fn nothing_reaches_the_network_or_other_files() {
    assert!(speedyspec_ffmpeg_sys::input_protocols().is_empty());
    for name in [
        "hls",
        "dash",
        "rtsp",
        "rtp",
        "sdp",
        "concat",
        "image2",
        "image2pipe",
        "alias_pix",
        "brender_pix",
        "ffmetadata",
        "imf",
        "tty",
        "rawvideo",
    ] {
        assert!(
            !speedyspec_ffmpeg_sys::has_demuxer(name),
            "denied demuxer {name} is built"
        );
    }
}

#[test]
fn version_matches_the_pin() {
    let script = include_str!("../../../scripts/build-ffmpeg.sh");
    let pinned = script
        .lines()
        .find_map(|line| line.strip_prefix("ffmpeg_version=\""))
        .expect("build-ffmpeg.sh pins ffmpeg_version")
        .trim_end_matches('"');
    assert_eq!(speedyspec_ffmpeg_sys::version(), pinned);
}

/// Every demuxer the build has is one a person reviewed: an FFmpeg upgrade that adds or renames a demuxer (a new
/// network or file-referencing format among them) fails here until `demuxers.txt` is updated on purpose.
#[test]
fn every_demuxer_is_reviewed() {
    let reviewed: Vec<&str> = include_str!("../demuxers.txt")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let built = speedyspec_ffmpeg_sys::demuxers();
    let unreviewed: Vec<_> = built
        .iter()
        .filter(|name| !reviewed.contains(name))
        .collect();
    let gone: Vec<_> = reviewed
        .iter()
        .filter(|name| !built.contains(name))
        .collect();
    assert!(
        unreviewed.is_empty(),
        "demuxers not in demuxers.txt: {unreviewed:?}"
    );
    assert!(
        gone.is_empty(),
        "demuxers.txt lists demuxers the build lacks: {gone:?}"
    );
}
