# Media corpus provenance

Every file in `corpus/` was made by `make-corpus.sh` (Alpine 3.22's FFmpeg in a throwaway container) from synthetic
sources only, so nothing here carries anyone else's rights:

- `src.wav` (not kept): FFmpeg `lavfi` `sine` 440 Hz left, 660 Hz right, 2 s, 44.1 kHz, 16-bit.
- covers (not kept): `lavfi` `testsrc2` frames, 64×48 JPEG and 48×64 PNG.
- video source: `lavfi` `testsrc2` 160×120, 10 fps, 2 s, with a 440 Hz `sine`.

| File | How |
|---|---|
| `tone.mp3` | `src.wav` → libmp3lame CBR 128k |
| `tone-vbr.mp3` | `src.wav` → libmp3lame `-q:a 4` (Xing header) |
| `tagged-v23.mp3` | libmp3lame 128k, ID3v2.3: title, artist, album, date 2024, genre, track 3, comment, JPEG cover |
| `tagged-v24.mp3` | mono 440 Hz, libmp3lame 96k, ID3v2.4: title, artist, PNG cover |
| `v1-only.mp3` | mono, libmp3lame 96k, no ID3v2; ID3v1 (title "Only v1", artist "Old") appended by the script |
| `latin1-v1.mp3` | `v1-only.mp3`'s audio with an ID3v1 title of the Latin-1 bytes `Caf\xe9` |
| `tone.m4a` | AAC 96k in MP4, title, artist, album, JPEG cover (attached picture) |
| `tone.flac` | FLAC, Vorbis comments title and artist, PNG cover (picture block) |
| `tone.ogg` | Vorbis q2, title, artist |
| `tone.opus` | Opus 64k, title |
| `tone.wav` / `tone-24.wav` | PCM s16le stereo / s24le mono |
| `tone.aiff` | PCM s16be |
| `tone.wma` | WMA v2 64k in ASF, title, artist |
| `tone.ra` | RealAudio 1.0 (`real_144`), 8 kHz mono, in RealMedia |
| `tone.ac3` | AC-3 128k |
| `tone.dts` | DTS (`dca`, `-strict -2`) |
| `tone.aac` | AAC in ADTS |
| `tone.mka` | Vorbis in Matroska, title |
| `note.mid` | the test kit's MIDI file (format 0, one note, track name "Song") |
| `bars.mp4` | H.264 + AAC, `creation_time=2024-05-06T07:08:09Z` |
| `bars.mov` | the same with brand `qt` |
| `bars.m4v` | H.264 + AAC, `ipod` muxer |
| `bars.3gp` | MPEG-4 Part 2 + AAC (the image's FFmpeg has no AMR encoder, so the H.263/AMR attempt falls back) |
| `bars.webm` | VP9 + Opus, creation time set |
| `bars.mkv` | H.264 + AAC, creation time set |
| `bars.ogv` | Theora + Vorbis |
| `bars.flv` | Sorenson H.263 (`flv1`) + MP3 |
| `bars.avi` | MPEG-4 Part 2 + MP3 |
| `bars.wmv` | WMV2 + WMA v2, creation time set |
| `bars.mpg` | MPEG-2 + MP2, program stream (`vob`) |
| `bars.ts` | MPEG-2 + MP2, transport stream |
| `truncated.mp4` | the first 2 KiB of `bars.mp4` |
| `garbage.mp3` | 4096 bytes of a fixed printable pattern |
| `empty.wav` | 0 bytes |

## Images (`images/`)

Made by `make-image-corpus.sh` (Alpine 3.22: FFmpeg, ImageMagick with HEIC/JXL/TIFF, libheif, libjxl, ExifTool)
from FFmpeg `lavfi` `testsrc2` frames, 64×48 (RGBA variants with alpha 128; animations 3 frames):

| Files | How |
|---|---|
| `baseline.jpg`, `progressive.jpg`, `gray.jpg`, `cmyk.jpg` | FFmpeg mjpeg; ImageMagick `-interlace JPEG`, `-colorspace Gray`, `-colorspace CMYK` |
| `iptc.jpg`, `exif-rotated.jpg` | `baseline.jpg` with ExifTool IPTC keywords and caption / `Orientation=6` |
| `rgb.png`, `rgba.png`, `palette.png`, `gray16.png`, `animated.png` | FFmpeg png (rgb24, rgba, pal8, gray16be), APNG |
| `static.gif`, `animated.gif` | FFmpeg gif |
| `rgb24.bmp`, `rgba32.bmp`, `pal8.bmp` | FFmpeg bmp |
| `rgb.tif`, `cmyk.tif` | FFmpeg tiff; ImageMagick CMYK |
| `lossy.webp`, `lossless.webp`, `alpha.webp`, `animated.webp` | FFmpeg libwebp |
| `lossy.avif`, `alpha.avif` | ImageMagick (libheif/AV1) |
| `animated.avif` | FFmpeg libaom-av1 (libsvtav1 fallback). No grid AVIF: none of the tools in the image makes one |
| `photo.heic` | `heif-enc` |
| `photo.jxl` | `cjxl -q 80` |
| `icon.ico` | ImageMagick, 32×32 and 16×16 entries |
| `photo.qoi`, `.pcx`, `.tga`, `.ppm`, `.pgm`, `.sgi`, `.hdr`, `.jp2` | FFmpeg's native encoders |
| `photo.psd` | ImageMagick |
| `truncated.jpg`, `empty.png`, `mislabelled.jpg` | the first 200 bytes of `baseline.jpg`; 0 bytes; a copy of `rgb.png` |

## `uploads/`

Every file in `uploads/` was made by `make-upload-corpus.sh` (Alpine 3.22's FFmpeg in a throwaway container) from the
same synthetic sources: a mono 440 Hz `sine` (44.1 kHz, 2 s unless noted) and `lavfi` `testsrc2` 160×120 at 10 fps.
They cover the formats the widened MIME list adds that `corpus/` doesn't already hold.

| File | How |
|---|---|
| `tone.eac3` | E-AC-3 96k |
| `tone.caf` | ALAC in Core Audio Format |
| `tone.wv` | WavPack |
| `tone.tta` | True Audio |
| `tone.au` | PCM s16be in Sun AU |
| `tone.mp2` | MPEG-1 Layer II 128k |
| `tone.w64` | PCM s16le in Sony Wave64 |
| `tone.voc` | 22.05 kHz PCM u8 in Creative Voice |
| `bars.m2ts` | MPEG-2 video and MP2 audio in MPEG-TS with 192-byte packets (`-mpegts_m2ts_mode 1`) |
| `bars.mxf` | MPEG-2 video 352×288 at 25 fps and PCM s16le 48 kHz in MXF |
| `bars.dv` | one 720×480 NTSC DV frame (`-target ntsc-dv`), no audio |
| `bars.vob` | MPEG-2 video and MP2 audio in a DVD program stream (`-f vob`) |
| `bars.m2v` | raw MPEG-2 video elementary stream |
| `bars.rm` | RealVideo 1.0 176×144 and RealAudio 1.0 8 kHz mono in RealMedia |
| `bars.ogm` | Theora and Vorbis in Ogg |
