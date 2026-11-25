# Where the native media functions differ from WordPress

The native `wp_read_audio_metadata()`, `wp_read_video_metadata()`, `wp_add_id3_tag_data()` and
`wp_get_media_creation_timestamp()` are compared with WordPress 7.2-RC2 and its bundled getID3 (`recorded.json`, made
by `record.php` over `corpus/`). Every difference the comparison allows is listed in `differences.json`, with the
reason here.

## WordPress versions

- The recordings are WordPress 7.2-RC2's (the compat suites' pinned release), with its bundled getID3
  1.9.28-202610301738. The native functions follow 7.2: `wp_read_video_metadata()` looks the creation timestamp up
  without 7.1's `if ( empty( $metadata['created_timestamp'] ) )` guard, and `wp_check_filetype_and_ext()` is 7.2's
  (see Content checks).

## Deliberate

- **Tags from every tag format.** WordPress copies titles, artists and the rest only from ID3v2 or ID3v1. When a file
  has neither, the native function copies them from its own tags (MP4, Vorbis, ASF, Matroska, RIFF, APE) under the
  same keys, through the same kses filtering (Jacob's ruling, 2026-11-22).

## Library differences

Each is a path in `differences.json`; the corpus test fails if one stops differing.

- **Bitrates.** getID3 computes bitrates from frame headers and Xing/VBRI/Ogg granule positions (a "CBR" LAME file
  comes out as a variable 129634.05… b/s); FFmpeg reports the container's or stream header's figure (128000). The
  `compression_ratio` getID3 derives from it differs with it.
- **Encoder options.** getID3 reads LAME and Vorbis encoder headers into `encoder_options` ("VBR", "CBR64",
  "Nominal bitrate: 96kbps"); FFmpeg doesn't expose them.
- **Vorbis vendor string.** getID3 reports an Ogg file's vendor string as `encoder`; FFmpeg drops the vendor and
  reports the ENCODER comment instead.
- **Default track language.** getID3 writes `"und"` as a Matroska track's language when the file names none; FFmpeg
  reports none.
- **QuickTime compressor name.** For a `qt`-brand file getID3 reports the sample description's compressor name
  ("Lavc60.31.102 libx264") as the video codec; FFmpeg doesn't expose it.
- **DTS resolution and Opus input rate.** getID3 reads a DTS header's PCM resolution (`bits_per_sample`) and an Opus
  header's original input rate (`sample_rate_input`); FFmpeg doesn't expose either without decoding.
- **Latin-1 tags.** ID3v1 text is Latin-1. WordPress returns the raw bytes (invalid UTF-8 in a JSON response); the
  native function decodes them to UTF-8.
- **Unreadable files.** For a file neither can parse, getID3 still guesses a MIME type or format from its first bytes
  (`garbage.mp3`, `truncated.mp4`); the native function reports only the size.
- **Files getID3 can't read.** getID3 returns only the size and a MIME guess for MPEG program and transport streams,
  ADTS AAC, RealAudio and MIDI, and only the Theora stream of an Ogg video; FFmpeg (or the MIDI reader) reads them,
  so the native functions return their full metadata.

## Images

The image functions are compared with WordPress 7.2-RC2 on PHP 8.5.12 (`recorded-images.json`, made by
`record-images.php` over `images/`); allowed differences are in `image-differences.json`.

- **The fast path is PHP's.** For every type PHP 8.5's `getimagesize()` reads (JPEG, PNG, GIF, BMP, TIFF, WebP, AVIF,
  HEIF, JPEG 2000, PSD, ICO) `wp_getimagesize()` returns PHP's own answer, `$image_info` and PHP 8.5's
  `width_unit`/`height_unit` included, and `wp_get_avif_info()` takes its figures from it: identical to WordPress.
- **More types.** For JPEG XL, QOI, PCX, PPM/PGM, SGI and Radiance HDR, WordPress returns `false` from
  `wp_getimagesize()` and `wp_get_image_mime()`; the native functions type them by content and size them through
  FFmpeg (`IMAGETYPE_UNKNOWN`, with `bits` and `channels` when FFmpeg knows them).
- **TGA** has no magic number, so the image functions return `false` for it, as WordPress does.

## Formats added to the MIME list

`wp_get_mime_types()` returns WordPress 7.2-RC2's list unchanged, in order, plus one row for every format below.
`wp_get_ext_types()` files each new extension under its kind, and `getimagesize_mimes_to_exts` gains the new image
types. Each added row's file in `compat/media` is read by the media reader as that kind (`uploads.rs`,
`every_added_row_is_read`). WordPress rejects these uploads; the native functions accept them (`mime-differences.json`).
Image types are also renamed to their real extension (`proper_filename`) when uploaded under another image extension,
as WordPress does for the types it knows.

| Extensions | MIME type | Kind | Corpus file |
|---|---|---|---|
| `jp2` | `image/jp2` | image | `images/photo.jp2` |
| `jxl` | `image/jxl` | image | `images/photo.jxl` |
| `qoi` | `image/qoi` | image | `images/photo.qoi` |
| `pcx` | `image/x-pcx` | image | `images/photo.pcx` |
| `ppm` | `image/x-portable-pixmap` | image | `images/photo.ppm` |
| `pgm` | `image/x-portable-graymap` | image | `images/photo.pgm` |
| `sgi` | `image/x-sgi` | image | `images/photo.sgi` |
| `hdr` | `image/vnd.radiance` | image | `images/photo.hdr` |
| `m2ts, mts` | `video/mp2t` | video | `uploads/bars.m2ts` |
| `mxf` | `application/mxf` | video | `uploads/bars.mxf` |
| `dv` | `video/dv` | video | `uploads/bars.dv` |
| `vob` | `video/mpeg` | video | `uploads/bars.vob` |
| `m2v, mpv` | `video/mpeg` | video | `uploads/bars.m2v` |
| `rm, rmvb` | `application/vnd.rn-realmedia` | video | `uploads/bars.rm` |
| `ogm` | `video/ogg` | video | `uploads/bars.ogm` |
| `aif, aiff, aifc` | `audio/aiff` | audio | `corpus/tone.aiff` |
| `ac3` | `audio/ac3` | audio | `corpus/tone.ac3` |
| `eac3, ec3` | `audio/eac3` | audio | `uploads/tone.eac3` |
| `opus` | `audio/opus` | audio | `corpus/tone.opus` |
| `caf` | `audio/x-caf` | audio | `uploads/tone.caf` |
| `wv` | `audio/x-wavpack` | audio | `uploads/tone.wv` |
| `tta` | `audio/x-tta` | audio | `uploads/tone.tta` |
| `au, snd` | `audio/basic` | audio | `uploads/tone.au` |
| `mp1, mp2, m2a` | `audio/mpeg` | audio | `uploads/tone.mp2` |
| `w64` | `audio/x-w64` | audio | `uploads/tone.w64` |
| `voc` | `audio/x-voc` | audio | `uploads/tone.voc` |

## Extensions left out

| Extension | Why |
|---|---|
| `.ts` | also TypeScript (and Qt translation) source |
| `.dts` | also Device Tree Source |
| `.tga` | no magic number to type it by, and no decoder in the build to size it |
| `.pbm` | its decoder is not in the build, so it can't be sized |
| `.rgb` | a generic name many raw formats share |

A plugin can still add any of them through the `mime_types` or `upload_mimes` filters.

## Content checks

`wp_check_filetype_and_ext()` is WordPress 7.2's function. It is compared with a recording of
WordPress 7.2-RC2 over every corpus file, under its own name and renamed to `.jpg`, `.png`, `.mp3`, `.mp4`, `.mka`,
`.txt` and `.pdf` (`recorded-mime.json`, made by `record-mime.php`). Its content type is fileinfo's, as in WordPress,
with one exception.

- **Audio that fileinfo calls video.** Fileinfo calls an audio-only Matroska file `video/x-matroska` and a WMA file
  `video/x-ms-asf`, so WordPress rejects `.mka` and `.wma` uploads. When fileinfo and the file's extension name
  different kinds (audio and video), the media reader reads the file. If the file holds only audio, as the extension
  says (or holds video, when the extension says video), the upload is accepted.
  The `wp_check_filetype_and_ext` filter's fifth argument (`$real_mime`) is then the reader's type with the
  extension's kind (`audio/x-matroska`), not fileinfo's. Where WordPress 7.2 already accepts audio that fileinfo calls
  `video/mp4` or `video/3gpp` under an `.mp3`-family extension, the reader is not consulted and `$real_mime` is
  fileinfo's, as in WordPress.
- **Other audio extensions.** WordPress compares only the top-level type of audio and video files, so it accepts an
  Ogg file named `.mp3`. Once the reader has confirmed a file as audio, the same rule accepts the WMA file named `.mp3`
  or `.mka`, and the Matroska file named `.mp3`.
- **Nothing else changes.** The reader is never consulted when fileinfo names a non-media type (`text/html`, `image/png`,
  `application/pdf`, …), when fileinfo already agrees, for stream-wrapper paths, or when the reader fails. Documents,
  files pretending to be media, and every upload WordPress accepts are judged exactly as WordPress judges them.
- **WordPress 7.2's changes.** The function follows WordPress 7.2, whose own tests run on it: an MP4 or 3GP file
  that fileinfo calls `video/mp4` or `video/3gpp` is accepted under an `.mp3`-family extension, and a TrueType font
  may be reported as `font/ttf` or `font/sfnt`. The recording is 7.2-RC2's, so neither is a listed difference.
- **RealAudio stays rejected.** Fileinfo calls `.ra` files `application/vnd.rn-realmedia`, which is not an audio type,
  so `.ra` uploads are rejected as WordPress rejects them.

## Metadata of the added formats

`wp_read_audio_metadata()` and `wp_read_video_metadata()` over `uploads/` are compared with WordPress 7.2-RC2 and getID3
(`recorded-uploads.json`); allowed differences are in `upload-differences.json`, plus the general reasons above
(bitrates, encoder options).

- **Formats getID3 doesn't read.** For DV, M2TS, MXF, RealVideo, VOB, CAF, TTA, Wave64 and WavPack, getID3 1.9.28
  gives WordPress only the file size; the native functions return the reader's length, dimensions and audio
  parameters. MXF's and DV's technical identifiers (uid, product, timecode) reach the metadata as tags, as every
  format's tags do.
- **Partly read.** getID3 reads an `.ogm` file's Theora stream but not its Vorbis stream, a raw MPEG-2 video stream's
  type but not its size, and AU and VOC files' type but no audio parameters; the native functions fill them in.
