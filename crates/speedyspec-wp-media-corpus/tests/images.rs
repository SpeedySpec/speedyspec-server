//! The image corpus read through the native image rules against WordPress 7.1.5 and PHP 8.5
//! (`compat/media/recorded-images.json`), except the paths `compat/media/image-differences.json` lists.
//!
//! On the fast path the facade returns PHP's own `getimagesize()`, so the recorded answer stands in for it here; this
//! test checks which files take it, and everything the native side computes itself.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use speedyspec_wp_media::images::{ImagePath, avif_info, reader_imagesize, sniff, webp_info};
use speedyspec_wp_media_contracts::{MediaReader, MediaSource, ProbeLimits, Stream, Tree};
use speedyspec_wp_media_ffmpeg::FfmpegReader;

fn compat() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/media")
}

fn plain(tree: &Tree) -> Value {
    match tree {
        Tree::Null => Value::Null,
        Tree::Bool(b) => json!(b),
        Tree::Int(i) => json!(i),
        Tree::Float(f) => json!(f),
        Tree::Str(s) => json!(s),
        Tree::Bytes(b) => json!(String::from_utf8_lossy(b)),
        Tree::List(l) => Value::Array(l.iter().map(plain).collect()),
        Tree::Map(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), plain(v))).collect()),
    }
}

fn allowed() -> Vec<(String, String)> {
    let list: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(compat().join("image-differences.json")).unwrap(),
    )
    .unwrap();
    list.iter()
        .map(|d| {
            (
                d["file"].as_str().unwrap().into(),
                d["field"].as_str().unwrap().into(),
            )
        })
        .collect()
}

fn head(path: &Path, n: usize) -> Vec<u8> {
    let mut bytes = std::fs::read(path).unwrap_or_default();
    bytes.truncate(n);
    bytes
}

/// What the native functions answer for one file: (wp_get_image_mime, wp_getimagesize, webp info, avif info).
fn native(path: &Path, recorded: &Value) -> [Value; 4] {
    let limits = ProbeLimits::default();
    let reader = FfmpegReader::new(&limits);
    let t = sniff(&head(path, 64));
    let mime = t.map_or(json!(false), |t| json!(t.mime));
    let size = match t {
        None => json!(false),
        // The facade asks PHP first; a 0×0 or failed answer falls through to the reader.
        Some(t) if t.path == ImagePath::Fast && recorded["getimagesize"].is_object() => {
            recorded["getimagesize"].clone()
        }
        Some(t) => reader
            .read(MediaSource::Path(path.to_path_buf()), &limits)
            .ok()
            .and_then(|info| reader_imagesize(&info, t.imagetype, t.mime))
            .map_or(json!(false), |tree| plain(&tree)),
    };
    let webp = if mime == "image/webp" {
        plain(&webp_info(&head(path, 40)))
    } else {
        json!({"width": false, "height": false, "type": false})
    };
    let php = &recorded["getimagesize"];
    let avif = if mime == "image/avif" && php.is_object() {
        // As the facade: PHP's getimagesize() first.
        json!({"width": php["0"], "height": php["1"], "bit_depth": php["bits"], "num_channels": php["channels"]})
    } else if mime == "image/avif" {
        let info = reader
            .read(MediaSource::Path(path.to_path_buf()), &limits)
            .ok();
        info.map_or(
            json!({"width": false, "height": false, "bit_depth": false, "num_channels": false}),
            |i| plain(&avif_info(&i)),
        )
    } else {
        json!({"width": false, "height": false, "bit_depth": false, "num_channels": false})
    };
    [mime, size, webp, avif]
}

fn run() -> (Vec<String>, Vec<String>) {
    let recorded: Value = serde_json::from_str(
        &std::fs::read_to_string(compat().join("recorded-images.json")).unwrap(),
    )
    .unwrap();
    let allowed = allowed();
    let mut used = vec![false; allowed.len()];
    let mut out = Vec::new();
    for (name, rec) in recorded["files"].as_object().unwrap() {
        let path = compat().join("images").join(name);
        let ours = native(&path, rec);
        for (field, expected, actual) in [
            ("wp_get_image_mime", &rec["wp_get_image_mime"], &ours[0]),
            ("wp_getimagesize", &rec["wp_getimagesize"], &ours[1]),
            ("wp_get_webp_info", &rec["wp_get_webp_info"], &ours[2]),
            ("wp_get_avif_info", &rec["wp_get_avif_info"], &ours[3]),
        ] {
            if expected == actual {
                continue;
            }
            match allowed
                .iter()
                .position(|(f, d)| (f == name || f == "*") && d == field)
            {
                Some(i) => used[i] = true,
                None => out.push(format!(
                    "{name}: {field}: WordPress {expected}, ours {actual}"
                )),
            }
        }
    }
    let stale = allowed
        .iter()
        .zip(&used)
        .filter(|(_, u)| !**u)
        .map(|((f, d), _)| format!("{f} {d}"))
        .collect();
    (out, stale)
}

#[test]
fn image_corpus_matches_wordpress_except_listed_differences() {
    let (mismatches, _) = run();
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn every_image_difference_is_still_needed() {
    let (_, stale) = run();
    assert!(
        stale.is_empty(),
        "image-differences.json entries that no longer differ:\n{}",
        stale.join("\n")
    );
}

/// Every fast-path type has a corpus file PHP's getimagesize() reads with FFmpeg's dimensions, and every corpus file
/// PHP reads that way is a fast-path type.
#[test]
fn fast_path_list_matches_the_recording() {
    let recorded: Value = serde_json::from_str(
        &std::fs::read_to_string(compat().join("recorded-images.json")).unwrap(),
    )
    .unwrap();
    let limits = ProbeLimits::default();
    let reader = FfmpegReader::new(&limits);
    let mut proven = Vec::new();
    for (name, rec) in recorded["files"].as_object().unwrap() {
        let path = compat().join("images").join(name);
        let Some(t) = sniff(&head(&path, 64)) else {
            continue;
        };
        let php = &rec["getimagesize"];
        let php_reads = php.is_object() && php["0"].as_u64().unwrap_or(0) > 0;
        let ffmpeg = reader
            .read(MediaSource::Path(path.clone()), &limits)
            .ok()
            .and_then(|info| {
                info.streams.iter().find_map(|s| match s {
                    Stream::Image(i) => Some((i.width?, i.height?)),
                    Stream::Video(v) => Some((v.width?, v.height?)),
                    _ => None,
                })
            });
        if php_reads {
            assert_eq!(
                t.path,
                ImagePath::Fast,
                "{name}: PHP reads it, so it should take the fast path"
            );
            if let Some((w, h)) = ffmpeg {
                // ICO's first entry and HEIF's tiling can differ from FFmpeg's choice; PHP's answer is WordPress's.
                if !matches!(t.mime, "image/vnd.microsoft.icon" | "image/heif") {
                    assert_eq!(
                        (php["0"].as_u64(), php["1"].as_u64()),
                        (Some(w.into()), Some(h.into())),
                        "{name}"
                    );
                }
            }
            proven.push(t.mime);
        } else if t.path == ImagePath::Fast && rec["exif_imagetype"].is_number() && php.is_boolean()
        {
            // A broken file of a fast type (truncated.jpg): PHP fails, which is fine.
        } else {
            assert_eq!(
                t.path,
                ImagePath::Reader,
                "{name}: PHP doesn't read it, so it can't take the fast path"
            );
        }
    }
    for t in speedyspec_wp_media::images::TYPES
        .iter()
        .filter(|t| t.path == ImagePath::Fast)
    {
        assert!(
            proven.contains(&t.mime),
            "{} is on the fast path with no corpus file proving PHP reads it",
            t.mime
        );
    }
}
