# Where the native media functions differ from WordPress

The native `wp_read_audio_metadata()`, `wp_read_video_metadata()`, `wp_add_id3_tag_data()` and
`wp_get_media_creation_timestamp()` are compared with WordPress 7.1.5 and its bundled getID3 (`recorded.json`, made
by `record.php` over `corpus/`). Every difference the comparison allows is listed in `differences.json`, with the
reason here.

## WordPress versions

- The recording is WordPress 7.1.5 (the compat suites' pinned release). Upstream trunk `ce78d2d6eb` (7.2) differs in
  one place: `wp_read_video_metadata()` no longer wraps the creation-timestamp lookup in
  `if ( empty( $metadata['created_timestamp'] ) )`. Nothing sets that key earlier in the function, so the output is
  the same.

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

The image functions are compared with WordPress 7.1.5 on PHP 8.5.12 (`recorded-images.json`, made by
`record-images.php` over `images/`); allowed differences are in `image-differences.json`.

- **The fast path is PHP's.** For every type PHP 8.5's `getimagesize()` reads (JPEG, PNG, GIF, BMP, TIFF, WebP, AVIF,
  HEIF, JPEG 2000, PSD, ICO) `wp_getimagesize()` returns PHP's own answer, `$image_info` and PHP 8.5's
  `width_unit`/`height_unit` included, and `wp_get_avif_info()` takes its figures from it: identical to WordPress.
- **More types.** For JPEG XL, QOI, PCX, PPM/PGM, SGI and Radiance HDR, WordPress returns `false` from
  `wp_getimagesize()` and `wp_get_image_mime()`; the native functions type them by content and size them through
  FFmpeg (`IMAGETYPE_UNKNOWN`, with `bits` and `channels` when FFmpeg knows them).
- **TGA** has no magic number, so the image functions return `false` for it, as WordPress does.
