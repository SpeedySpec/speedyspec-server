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
