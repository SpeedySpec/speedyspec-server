//! WordPress's `wp_read_audio_metadata()`, `wp_read_video_metadata()`, `wp_add_id3_tag_data()` and
//! `wp_get_media_creation_timestamp()` (WordPress 7.1.5), over the media reader and the getID3-shaped view.

use std::path::Path;

use speedyspec_wp_media_contracts::{
    MediaReader, MediaSource, ProbeLimits, TagSanitizer, TimeParser, Tree,
};

use crate::getid3::getid3_view;
use crate::neutral::{SCHEMA, neutral};
use crate::php::{empty, entries, get, int, isset, path, reset, set, unset};

/// What `wp_read_*_metadata()` passes to its filter: the metadata (the filter's value), getID3's `fileformat` and the
/// raw data (its third and fourth arguments). The PHP bridge applies the filter.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub metadata: Tree,
    pub file_format: Option<Vec<u8>>,
    pub data: Tree,
}

/// The raw data for `path`: the getID3-shaped view with `speedyspec` holding the neutral description, or, for a file
/// the reader couldn't read, what getID3 gives for one (its size) and the error.
fn data(path: &Path, reader: &dyn MediaReader, limits: &ProbeLimits) -> Tree {
    match reader.read(MediaSource::Path(path.to_path_buf()), limits) {
        Ok(info) => {
            let mut view = getid3_view(&info);
            set(&mut view, "speedyspec", neutral(&info));
            view
        }
        Err(err) => {
            let size = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);
            Tree::Map(vec![
                ("filesize".into(), Tree::Int(size)),
                (
                    "speedyspec".into(),
                    Tree::Map(vec![
                        ("schema".into(), Tree::Int(SCHEMA)),
                        ("error".into(), Tree::Bytes(err.to_string().into_bytes())),
                    ]),
                ),
            ])
        }
    }
}

fn copy(metadata: &mut Tree, data: &Tree, from: &[&str], to: &str, cast: fn(&Tree) -> Tree) {
    let value = path(data, from);
    if !empty(value) {
        set(metadata, to, cast(value.expect("not empty")));
    }
}

fn same(v: &Tree) -> Tree {
    v.clone()
}

fn to_int(v: &Tree) -> Tree {
    Tree::Int(int(v))
}

/// PHP's `(int) round($seconds)`.
fn rounded(v: &Tree) -> Tree {
    let f = match v {
        Tree::Float(f) => *f,
        other => int(other) as f64,
    };
    Tree::Int(f.round() as i64)
}

fn finish(
    mut metadata: Tree,
    data: Tree,
    sanitizer: &dyn TagSanitizer,
    time: &dyn TimeParser,
) -> Outcome {
    if empty(get(Some(&metadata), "created_timestamp"))
        && let Some(ts) = media_creation_timestamp(&data, time)
    {
        set(&mut metadata, "created_timestamp", Tree::Int(ts));
    }
    add_id3_tag_data(&mut metadata, &data, sanitizer);
    let file_format = get(Some(&metadata), "fileformat")
        .and_then(Tree::bytes)
        .map(<[u8]>::to_vec);
    Outcome {
        metadata,
        file_format,
        data,
    }
}

/// `wp_read_video_metadata($file)`: `None` where WordPress returns `false` (the file doesn't exist).
pub fn read_video_metadata(
    file: &Path,
    reader: &dyn MediaReader,
    limits: &ProbeLimits,
    sanitizer: &dyn TagSanitizer,
    time: &dyn TimeParser,
) -> Option<Outcome> {
    if !file.exists() {
        return None;
    }
    let data = data(file, reader, limits);
    let mut m = Tree::Map(Vec::new());
    if isset(path(&data, &["video", "lossless"])) {
        set(
            &mut m,
            "lossless",
            path(&data, &["video", "lossless"]).cloned().expect("isset"),
        );
    }
    copy(&mut m, &data, &["video", "bitrate"], "bitrate", to_int);
    copy(
        &mut m,
        &data,
        &["video", "bitrate_mode"],
        "bitrate_mode",
        same,
    );
    copy(&mut m, &data, &["filesize"], "filesize", to_int);
    copy(&mut m, &data, &["mime_type"], "mime_type", same);
    copy(&mut m, &data, &["playtime_seconds"], "length", rounded);
    copy(
        &mut m,
        &data,
        &["playtime_string"],
        "length_formatted",
        same,
    );
    copy(&mut m, &data, &["video", "resolution_x"], "width", to_int);
    copy(&mut m, &data, &["video", "resolution_y"], "height", to_int);
    copy(&mut m, &data, &["fileformat"], "fileformat", same);
    copy(&mut m, &data, &["video", "dataformat"], "dataformat", same);
    copy(&mut m, &data, &["video", "encoder"], "encoder", same);
    copy(&mut m, &data, &["video", "codec"], "codec", same);
    if !empty(get(Some(&data), "audio")) {
        let mut audio = get(Some(&data), "audio").cloned().expect("not empty");
        unset(&mut audio, "streams");
        set(&mut m, "audio", audio);
    }
    Some(finish(m, data, sanitizer, time))
}

/// `wp_read_audio_metadata($file)`: `None` where WordPress returns `false` (the file doesn't exist).
pub fn read_audio_metadata(
    file: &Path,
    reader: &dyn MediaReader,
    limits: &ProbeLimits,
    sanitizer: &dyn TagSanitizer,
    time: &dyn TimeParser,
) -> Option<Outcome> {
    if !file.exists() {
        return None;
    }
    let data = data(file, reader, limits);
    let mut m = Tree::Map(Vec::new());
    if !empty(get(Some(&data), "audio")) {
        m = get(Some(&data), "audio").cloned().expect("not empty");
        unset(&mut m, "streams");
    }
    copy(&mut m, &data, &["fileformat"], "fileformat", same);
    copy(&mut m, &data, &["filesize"], "filesize", to_int);
    copy(&mut m, &data, &["mime_type"], "mime_type", same);
    copy(&mut m, &data, &["playtime_seconds"], "length", rounded);
    copy(
        &mut m,
        &data,
        &["playtime_string"],
        "length_formatted",
        same,
    );
    Some(finish(m, data, sanitizer, time))
}

/// One comment value as WordPress copies it: the first element of a list through `wp_kses_post_deep()`, a string
/// through `wp_kses_post()`.
fn comment_value(list: &Tree, sanitizer: &dyn TagSanitizer) -> Tree {
    match list {
        Tree::List(_) | Tree::Map(_) => sanitizer.post_deep(reset(list).unwrap_or(&Tree::Null)),
        other => Tree::Bytes(sanitizer.post(other.bytes().unwrap_or(&[]))),
    }
}

fn copy_comments(
    metadata: &mut Tree,
    comments: &Tree,
    sanitizer: &dyn TagSanitizer,
    skip_picture: bool,
) {
    for (key, list) in entries(comments) {
        if key == "length" || (skip_picture && key == "picture") || empty(Some(list)) {
            continue;
        }
        let mut value = comment_value(list, sanitizer);
        // WordPress's fix for a getID3 byte-stream bug.
        if key == "terms_of_use"
            && let Some(text) = value.bytes().filter(|b| b.starts_with(b"yright notice."))
        {
            value = Tree::Bytes([&b"Cop"[..], text].concat());
        }
        set(metadata, &key, value);
    }
}

/// `wp_add_id3_tag_data(&$metadata, $data)`. Deliberate difference: when neither ID3v2 nor ID3v1 comments exist, the
/// same copy runs over the merged `comments` (every other tag format), pictures excepted.
pub fn add_id3_tag_data(metadata: &mut Tree, data: &Tree, sanitizer: &dyn TagSanitizer) {
    let mut copied = false;
    for version in ["id3v2", "id3v1"] {
        let comments = path(data, &[version, "comments"]);
        if !empty(comments) {
            copy_comments(metadata, comments.expect("not empty"), sanitizer, false);
            copied = true;
            break;
        }
    }
    if !copied && let Some(comments) = get(Some(data), "comments").filter(|c| !empty(Some(c))) {
        copy_comments(metadata, comments, sanitizer, true);
    }

    let apic = path(data, &["id3v2", "APIC"]);
    if !empty(apic) {
        let image = reset(apic.expect("not empty"))
            .cloned()
            .unwrap_or(Tree::Null);
        if !empty(get(Some(&image), "data")) {
            let field = |k: &str| get(Some(&image), k).cloned().unwrap_or(Tree::Null);
            set(
                metadata,
                "image",
                Tree::Map(vec![
                    ("data".into(), field("data")),
                    ("mime".into(), field("image_mime")),
                    ("width".into(), field("image_width")),
                    ("height".into(), field("image_height")),
                ]),
            );
        }
    } else if let Some(pictures) = path(data, &["comments", "picture"]).filter(|p| !empty(Some(p)))
    {
        let image = reset(pictures).cloned().unwrap_or(Tree::Null);
        if !empty(get(Some(&image), "data")) {
            let field = |k: &str| get(Some(&image), k).cloned().unwrap_or(Tree::Null);
            set(
                metadata,
                "image",
                Tree::Map(vec![
                    ("data".into(), field("data")),
                    ("mime".into(), field("image_mime")),
                ]),
            );
        }
    }
}

/// `wp_get_media_creation_timestamp($metadata)`: `None` where WordPress returns `false`.
pub fn media_creation_timestamp(data: &Tree, time: &dyn TimeParser) -> Option<i64> {
    let format = get(Some(data), "fileformat");
    if empty(format) {
        return None;
    }
    match format.and_then(Tree::bytes)? {
        b"asf" => path(
            data,
            &["asf", "file_properties_object", "creation_date_unix"],
        )
        .filter(|v| isset(Some(v)))
        .map(int),
        b"matroska" | b"webm" => {
            if let Some(text) = path(data, &["matroska", "comments", "creation_time", "0"])
                .filter(|v| isset(Some(v)))
            {
                time.strtotime(text.bytes().unwrap_or(&[]))
            } else {
                path(data, &["matroska", "info", "0", "DateUTC_unix"])
                    .filter(|v| isset(Some(v)))
                    .map(int)
            }
        }
        b"quicktime" | b"mp4" => path(
            data,
            &["quicktime", "moov", "subatoms", "0", "creation_time_unix"],
        )
        .filter(|v| isset(Some(v)))
        .map(int),
        _ => None,
    }
}
