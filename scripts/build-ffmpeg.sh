#!/usr/bin/env bash
# Builds the static FFmpeg the runtime reads media metadata with (libavformat, libavcodec, libavutil only) from a
# pinned, checksum-checked source tarball, into $SPEEDYSPEC_FFMPEG_DIR/prefix (default .php-build/ffmpeg/prefix).
#
#   scripts/build-ffmpeg.sh                    build, or say it is up to date
#   scripts/build-ffmpeg.sh --print-configure  print the configure arguments, one per line, and build nothing
#
# The build reads metadata only: no programs, no network, no protocols (the runtime hands FFmpeg its own reader), no
# encoders or muxers, no external libraries, and nothing GPL, version 3 or nonfree, so FFmpeg stays LGPL 2.1 or later
# (docs/licensing.md). Every demuxer that reads one byte stream is in; the deny list below keeps out the ones that
# reach the network or other files, accept any bytes, or read subtitles. zlib comes from the static PHP's own libz.a
# ($SPEEDYSPEC_PHP_PREFIX, default .php-build/buildroot), so one zlib serves PHP and FFmpeg.
#
# build-php.sh runs this at its end. Run it alone to rebuild FFmpeg against an existing PHP build.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# FFmpeg 9.1 "Gauss". The checksum was checked against the release's signature (FFmpeg release signing key
# <ffmpeg-devel@ffmpeg.org>, fingerprint FCF986EA15E6E293A5644F10B4322F04D67658D8, from https://ffmpeg.org/download.html).
ffmpeg_version="9.1"
ffmpeg_url="https://ffmpeg.org/releases/ffmpeg-$ffmpeg_version.tar.xz"
ffmpeg_sha256="1a4d394ae63d190a526cd052eb4415c156b5c5e2df37efab41a7de0b8fde2433"

# The oldest macOS the binary runs on; must match .cargo/config.toml and build-php.sh.
macos_deployment_target="12.0"

# sha256_of and fetch_pinned.
# shellcheck source=lib/pinned-downloads.sh
source "$repo/scripts/lib/pinned-downloads.sh"

php_prefix="${SPEEDYSPEC_PHP_PREFIX:-$repo/.php-build/buildroot}"
ffmpeg_dir="${SPEEDYSPEC_FFMPEG_DIR:-$repo/.php-build/ffmpeg}"
tarball="$ffmpeg_dir/downloads/ffmpeg-$ffmpeg_version.tar.xz"
src="$ffmpeg_dir/src"
prefix="$ffmpeg_dir/prefix"

fixed_flags=(
    --enable-small
    --disable-everything
    --disable-programs
    --disable-doc
    --disable-network
    --disable-autodetect
    --disable-debug
    --enable-static
    --disable-shared
    --enable-pic
    --disable-protocols
    --disable-avdevice
    --disable-avfilter
    --disable-swscale
    --disable-swresample
    # Inline assembly stays; only the nasm-assembled x86 files go, so no build needs nasm.
    --disable-x86asm
    --enable-zlib
)

# Demuxers that must never be reachable from an upload. Names the pinned version doesn't have are skipped.
deny_demuxers=(
    # Network, streaming and manifest formats: they open URLs.
    hls dash rtsp rtp sdp sap srtp webm_dash_manifest
    # Formats that open other files: playlists, file sequences, metadata sidecars, packaging lists.
    concat image2 ffmetadata imf
    # A generic image pipe needs a codec hint; the per-format *_pipe demuxers cover images.
    image2pipe
    # Accept any bytes: no header to recognize, so anything "probes" as them.
    data bin tty rawvideo
    # External libraries and scripting hosts (off with --disable-autodetect anyway).
    lavfi libgme libopenmpt libmodplug avisynth vapoursynth
    # Subtitles and captions: not media WordPress stores.
    srt ass webvtt microdvd subviewer subviewer1 sami realtext jacosub mpl2 mpsub pjs stl vplayer aqtitle lrc scc
    mcc dvbsub dvbtxt pgssub
)
# Header-less PCM: every pcm_* demuxer, matched by prefix.
deny_demuxer_prefixes=(pcm_)

# Decoders avformat_find_stream_info needs for stream properties the demuxers and parsers don't fill: image width,
# height and pixel format. Provisional: plan 3's corpus trims or grows this list.
decoders=(
    mjpeg jpegls png apng gif bmp tiff webp jpeg2000 qoi pcx targa ppm pgm pgmyuv pbm pam sgi sunrast xbm xpm xwd dds
    psd hdr
)

denied() {
    local name="$1" entry
    for entry in "${deny_demuxers[@]}"; do
        [ "$name" = "$entry" ] && return 0
    done
    for entry in "${deny_demuxer_prefixes[@]}"; do
        case "$name" in "$entry"*) return 0 ;; esac
    done
    return 1
}

# Lists one configure component kind (demuxers, parsers, decoders) from the extracted source, one name per line.
list_components() {
    (cd "$src" && ./configure --list-"$1") | tr -s ' \t' '\n' | sed '/^$/d' | sort -u
}

fetch_and_extract() {
    if [ ! -f "$tarball" ] || [ "$(sha256_of "$tarball")" != "$ffmpeg_sha256" ]; then
        fetch_pinned "$ffmpeg_url" "$ffmpeg_sha256" "$tarball" "ffmpeg-$ffmpeg_version.tar.xz"
    fi
    if [ ! -x "$src/configure" ]; then
        rm -rf "$src"
        mkdir -p "$src"
        tar -xJf "$tarball" -C "$src" --strip-components=1
    fi
}

# Prints the configure arguments, one per line (needs the extracted source for the component lists).
configure_args() {
    local name all kept=() skipped=() known
    printf '%s\n' "${fixed_flags[@]}"
    if [ "$(uname -s)" = "Darwin" ]; then
        printf '%s\n' "--extra-cflags=-I$php_prefix/include -mmacosx-version-min=$macos_deployment_target"
        printf '%s\n' "--extra-ldflags=-L$php_prefix/lib -mmacosx-version-min=$macos_deployment_target"
    else
        printf '%s\n' "--extra-cflags=-I$php_prefix/include"
        printf '%s\n' "--extra-ldflags=-L$php_prefix/lib"
    fi
    all="$(list_components demuxers)"
    while IFS= read -r name; do
        if denied "$name"; then skipped+=("$name"); else kept+=("$name"); fi
    done <<<"$all"
    for name in "${deny_demuxers[@]}"; do
        if ! grep -qx "$name" <<<"$all"; then
            echo "build-ffmpeg.sh: deny-list entry $name is not a demuxer in FFmpeg $ffmpeg_version; skipped" >&2
        fi
    done
    printf '%s\n' "--enable-demuxer=$(IFS=,; echo "${kept[*]}")"
    printf '%s\n' "--enable-parser=$(list_components parsers | paste -sd, -)"
    known="$(list_components decoders)"
    for name in "${decoders[@]}"; do
        if ! grep -qx "$name" <<<"$known"; then
            echo "build-ffmpeg.sh: decoder $name is not in FFmpeg $ffmpeg_version" >&2
            exit 1
        fi
    done
    printf '%s\n' "--enable-decoder=$(IFS=,; echo "${decoders[*]}")"
}

# Everything the build depends on besides the pinned source, which fixes the component lists.
stamp_content() {
    printf 'ffmpeg: %s sha256=%s\n' "$ffmpeg_version" "$ffmpeg_sha256"
    printf 'flags: %s\n' "${fixed_flags[*]}"
    printf 'deny: %s %s*\n' "${deny_demuxers[*]}" "${deny_demuxer_prefixes[*]}"
    printf 'decoders: %s\n' "${decoders[*]}"
    printf 'php: %s\n' "$php_prefix"
    if [ "$(uname -s)" = "Darwin" ]; then printf 'macos: %s\n' "$macos_deployment_target"; fi
}

fail() {
    echo "build-ffmpeg.sh: $1" >&2
    exit 1
}

# Fails unless the configured build is LGPL only, has no protocol, and leaves zlib to PHP's archive.
check_build() {
    local mak="$src/ffbuild/config.mak" flag lib
    for flag in CONFIG_GPL CONFIG_VERSION3 CONFIG_NONFREE; do
        if grep -q "^$flag=yes" "$mak"; then fail "$flag is enabled; FFmpeg must stay LGPL 2.1"; fi
    done
    if grep -q '^CONFIG_[A-Z0-9_]*_PROTOCOL=yes' "$mak"; then
        fail "protocols are enabled: $(grep -o '^CONFIG_[A-Z0-9_]*_PROTOCOL' "$mak" | tr '\n' ' ')"
    fi
    for lib in avformat avcodec avutil; do
        [ -f "$prefix/lib/lib$lib.a" ] || fail "$prefix/lib/lib$lib.a was not produced"
    done
    for lib in avformat avcodec avutil; do
        if nm -g "$prefix/lib/lib$lib.a" 2>/dev/null | grep -Eq ' T _?(inflate|deflate)$'; then
            fail "lib$lib.a defines zlib's functions; zlib must come from $php_prefix/lib/libz.a"
        fi
    done
}

if [ "${1:-}" = "--print-configure" ]; then
    fetch_and_extract
    configure_args
    exit 0
fi
[ "$#" -eq 0 ] || fail "usage: scripts/build-ffmpeg.sh [--print-configure]"

[ -f "$php_prefix/lib/libz.a" ] && [ -f "$php_prefix/include/zlib.h" ] \
    || fail "no zlib in $php_prefix (lib/libz.a, include/zlib.h); run scripts/build-php.sh first"

stamp="$(stamp_content)"
if [ -f "$prefix/.stamp" ] && [ "$(cat "$prefix/.stamp")" = "$stamp" ]; then
    echo "build-ffmpeg.sh: FFmpeg $ffmpeg_version is up to date in $prefix"
    exit 0
fi

fetch_and_extract
args=()
while IFS= read -r line; do args+=("$line"); done < <(configure_args)

rm -rf "$prefix"
jobs="$(nproc 2>/dev/null || sysctl -n hw.ncpu)"
(
    cd "$src"
    ./configure --prefix="$prefix" "${args[@]}" > "$ffmpeg_dir/configure.log" 2>&1 \
        || { tail -40 "$ffmpeg_dir/configure.log" >&2; exit 1; }
    make -j"$jobs" > "$ffmpeg_dir/make.log" 2>&1 || { tail -40 "$ffmpeg_dir/make.log" >&2; exit 1; }
    make install > /dev/null
)
check_build

printf '%s\n' "${args[@]}" > "$prefix/configure-line.txt"
printf '%s\n' "$stamp" > "$prefix/.stamp"
rm -rf "$src" "$ffmpeg_dir/configure.log" "$ffmpeg_dir/make.log"
echo "build-ffmpeg.sh: built FFmpeg $ffmpeg_version in $prefix"
