#!/usr/bin/env bash
# Generates compat/media/uploads: one small file per audio or video format the widened MIME list adds that the main
# corpus doesn't already hold, all from synthetic sources (FFmpeg's lavfi tones and test patterns), so nothing is
# downloaded and nothing carries a third party's rights. Runs one throwaway Alpine container with a full FFmpeg.
# PROVENANCE.md lists each file's command; rerun this script to regenerate (output need not be byte-identical).
#
#   compat/media/make-upload-corpus.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out="$here/uploads"
mkdir -p "$out"
rm -f "$out"/*

docker run --rm -v "$out:/out" -w /out alpine:3.22 sh -euc '
apk add -q --no-cache ffmpeg >/dev/null
F="ffmpeg -nostdin -hide_banner -loglevel error -y"
TONE="-f lavfi -i sine=frequency=440:duration=2:sample_rate=44100"
BARS="-f lavfi -i testsrc2=size=160x120:rate=10:duration=2"

# Audio.
$F $TONE -c:a eac3 -b:a 96k tone.eac3
$F $TONE -c:a alac tone.caf
$F $TONE -c:a wavpack tone.wv
$F $TONE -c:a tta tone.tta
$F $TONE -c:a pcm_s16be tone.au
$F $TONE -c:a mp2 -b:a 128k tone.mp2
$F $TONE -c:a pcm_s16le tone.w64
$F -f lavfi -i sine=frequency=440:duration=2:sample_rate=22050 -c:a pcm_u8 tone.voc

# Video.
$F $BARS $TONE -map 0:v -map 1:a -c:v mpeg2video -c:a mp2 -f mpegts -mpegts_m2ts_mode 1 bars.m2ts
$F $BARS $TONE -map 0:v -map 1:a -c:v mpeg2video -r 25 -s 352x288 -pix_fmt yuv420p -c:a pcm_s16le -ar 48000 bars.mxf
$F -f lavfi -i testsrc2=size=720x480:rate=30000/1001:duration=0.1 -frames:v 1 -target ntsc-dv -an bars.dv
$F $BARS $TONE -map 0:v -map 1:a -c:v mpeg2video -c:a mp2 -f vob bars.vob
$F $BARS -c:v mpeg2video -f mpeg2video bars.m2v
$F $BARS -f lavfi -i sine=frequency=440:duration=2:sample_rate=8000 -map 0:v -map 1:a -c:v rv10 -s 176x144 -c:a real_144 -ac 1 bars.rm
$F $BARS $TONE -map 0:v -map 1:a -c:v libtheora -q:v 3 -c:a libvorbis -f ogg bars.ogm
'
du -sh "$out"
