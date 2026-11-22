#!/usr/bin/env bash
# Generates compat/media/corpus: small audio and video files in every format WordPress accepts, plus broken ones, all
# from synthetic sources (FFmpeg's lavfi tones and test patterns, the test kit's MIDI), so nothing is downloaded and
# nothing carries a third party's rights. Runs one throwaway Alpine container with a full FFmpeg. PROVENANCE.md lists
# each file's command; rerun this script to regenerate (output need not be byte-identical).
#
#   compat/media/make-corpus.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
out="$here/corpus"
mkdir -p "$out"
rm -f "$out"/*

# The test kit's MIDI file and the cover images come from the host side.
(cd "$repo" && cargo run -q -p speedyspec-wp-media-testkit --example write_fixtures "$out/.kit")
cp "$out/.kit/note.mid" "$out/note.mid"
rm -rf "$out/.kit"

docker run --rm -v "$out:/out" -w /out alpine:3.22 sh -euc '
apk add -q --no-cache ffmpeg >/dev/null
F="ffmpeg -nostdin -hide_banner -loglevel error -y"
TONE="-f lavfi -i sine=frequency=440:duration=2:sample_rate=44100"
STEREO="-i src.wav"
BARS="-f lavfi -i testsrc2=size=160x120:rate=10:duration=2"
CT="-metadata creation_time=2024-05-06T07:08:09Z"

# A stereo source (440 Hz left, 660 Hz right) every stereo file is encoded from.
$F -f lavfi -i sine=frequency=440:duration=2:sample_rate=44100 -f lavfi -i sine=frequency=660:duration=2:sample_rate=44100 \
  -filter_complex "[0:a][1:a]amerge=inputs=2" -c:a pcm_s16le src.wav

# Covers.
$F -f lavfi -i testsrc2=size=64x48:duration=1 -frames:v 1 cover.jpg
$F -f lavfi -i testsrc2=size=48x64:duration=1 -frames:v 1 cover.png

# Audio.
$F $STEREO -c:a libmp3lame -b:a 128k tone.mp3
$F $STEREO -c:a libmp3lame -q:a 4 tone-vbr.mp3
$F $STEREO -i cover.jpg -map 0:a -map 1:v -c:a libmp3lame -b:a 128k -c:v copy -id3v2_version 3 \
  -metadata title="Tagged v2.3" -metadata artist="Corpus Artist" -metadata album="Corpus Album" -metadata date=2024 \
  -metadata genre=Ambient -metadata track=3 -metadata comment="A comment" \
  -metadata:s:v title="Album cover" -metadata:s:v comment="Cover (front)" tagged-v23.mp3
$F $TONE -i cover.png -map 0:a -map 1:v -c:a libmp3lame -b:a 96k -c:v copy -id3v2_version 4 \
  -metadata title="Tagged v2.4" -metadata artist="Corpus Artist" \
  -metadata:s:v comment="Cover (front)" tagged-v24.mp3
$F $TONE -c:a libmp3lame -b:a 96k -id3v2_version 0 -write_id3v1 1 -metadata title="Only v1" -metadata artist="Old" v1-only.mp3
$F $STEREO -i cover.jpg -map 0:a -map 1:v -c:a aac -b:a 96k -c:v copy -disposition:v attached_pic \
  -metadata title="M4A Title" -metadata artist="M4A Artist" -metadata album="M4A Album" tone.m4a
$F $STEREO -i cover.png -map 0:a -map 1:v -c:a flac -c:v copy -disposition:v attached_pic \
  -metadata title="FLAC Title" -metadata artist="FLAC Artist" tone.flac
$F $STEREO -c:a libvorbis -q:a 2 -metadata title="Ogg Title" -metadata artist="Ogg Artist" tone.ogg
$F $STEREO -c:a libopus -b:a 64k -metadata title="Opus Title" tone.opus
$F $STEREO -c:a pcm_s16le tone.wav
$F $TONE -c:a pcm_s24le tone-24.wav
$F $STEREO -c:a pcm_s16be tone.aiff
$F $STEREO -c:a wmav2 -b:a 64k -metadata title="WMA Title" -metadata artist="WMA Artist" tone.wma
$F -f lavfi -i sine=frequency=440:duration=2:sample_rate=8000 -c:a real_144 -ac 1 tone.ra
$F $STEREO -c:a ac3 -b:a 128k tone.ac3
$F $STEREO -strict -2 -c:a dca tone.dts
$F $STEREO -c:a aac -b:a 96k -f adts tone.aac
$F $STEREO -c:a libvorbis -metadata title="MKA Title" tone.mka

# Video.
$F $BARS $TONE -map 0:v -map 1:a -c:v libx264 -preset veryfast -pix_fmt yuv420p -c:a aac $CT bars.mp4
$F $BARS $TONE -map 0:v -map 1:a -c:v libx264 -preset veryfast -pix_fmt yuv420p -c:a aac -brand qt -f mov $CT bars.mov
$F $BARS $TONE -map 0:v -map 1:a -c:v libx264 -preset veryfast -pix_fmt yuv420p -c:a aac -f ipod bars.m4v
$F $BARS -f lavfi -i sine=frequency=440:duration=2:sample_rate=8000 -map 0:v -map 1:a -c:v h263 -s 176x144 -c:a libopencore_amrnb -ac 1 -b:a 12.2k bars.3gp \
  || $F $BARS $TONE -map 0:v -map 1:a -c:v mpeg4 -c:a aac bars.3gp
$F $BARS $TONE -map 0:v -map 1:a -c:v libvpx-vp9 -b:v 100k -c:a libopus $CT bars.webm
$F $BARS $TONE -map 0:v -map 1:a -c:v libx264 -preset veryfast -pix_fmt yuv420p -c:a aac $CT bars.mkv
$F $BARS $TONE -map 0:v -map 1:a -c:v libtheora -q:v 3 -c:a libvorbis bars.ogv
$F $BARS $TONE -map 0:v -map 1:a -c:v flv1 -c:a libmp3lame -ar 44100 bars.flv
$F $BARS $TONE -map 0:v -map 1:a -c:v mpeg4 -c:a libmp3lame bars.avi
$F $BARS $TONE -map 0:v -map 1:a -c:v wmv2 -c:a wmav2 $CT bars.wmv
$F $BARS $TONE -map 0:v -map 1:a -c:v mpeg2video -c:a mp2 -f vob bars.mpg
$F $BARS $TONE -map 0:v -map 1:a -c:v mpeg2video -c:a mp2 bars.ts

# Broken.
head -c 2048 bars.mp4 > truncated.mp4
awk "BEGIN { for (i = 0; i < 4096; i++) printf \"%c\", 33 + (i * 7) % 90 }" > garbage.mp3
: > empty.wav

rm cover.jpg cover.png src.wav
'
# ID3v1 tags are appended here (FFmpeg's mp3 muxer in this image doesn't write them): v1-only.mp3 gets title "Only v1"
# and artist "Old"; latin1-v1.mp3 is a copy whose title is the Latin-1 bytes "Caf\xe9".
python3 -I - "$out" <<'PY'
import sys, pathlib
d = pathlib.Path(sys.argv[1])
def v1(title, artist):
    tag = bytearray(128)
    tag[0:3] = b"TAG"
    tag[3:33] = title.ljust(30, b"\0")
    tag[33:63] = artist.ljust(30, b"\0")
    tag[127] = 255
    return bytes(tag)
audio = (d / "v1-only.mp3").read_bytes()
if audio[-128:-125] == b"TAG":
    audio = audio[:-128]
(d / "v1-only.mp3").write_bytes(audio + v1(b"Only v1", b"Old"))
(d / "latin1-v1.mp3").write_bytes(audio + v1(b"Caf\xe9", b"Old"))
PY
du -sh "$out"
