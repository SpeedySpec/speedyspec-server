//! WordPress's MIME tables, widened to the formats the media reader reads.

use std::collections::HashMap;

use speedyspec_wp_media::mime::{
    ADDED, Kind, LEFT_OUT, WORDPRESS_EXT_TYPES, WORDPRESS_MIME_TYPES, ext_types,
    image_mimes_to_exts, mime_types,
};

fn exts(key: &str) -> impl Iterator<Item = &str> {
    key.split('|')
}

#[test]
fn wordpress_rows_unchanged_and_in_order() {
    let added: Vec<&str> = ADDED.iter().map(|r| r.exts).collect();
    let kept: Vec<(&str, &str)> = mime_types()
        .into_iter()
        .filter(|(key, _)| !added.contains(key))
        .collect();
    assert_eq!(kept, WORDPRESS_MIME_TYPES);
    assert_eq!(mime_types().len(), WORDPRESS_MIME_TYPES.len() + ADDED.len());
}

#[test]
fn added_rows_follow_their_kind() {
    let types = mime_types();
    let at = |key: &str| types.iter().position(|(k, _)| *k == key).unwrap();
    for (kind, anchor) in [
        (Kind::Image, "heifs"),
        (Kind::Video, "3g2|3gp2"),
        (Kind::Audio, "mka"),
    ] {
        let rows: Vec<&str> = ADDED
            .iter()
            .filter(|r| r.kind == kind)
            .map(|r| r.exts)
            .collect();
        let start = at(anchor) + 1;
        let placed: Vec<&str> = types[start..start + rows.len()]
            .iter()
            .map(|(k, _)| *k)
            .collect();
        assert_eq!(placed, rows, "{kind:?} rows after {anchor}");
    }
}

#[test]
fn no_extension_in_two_keys() {
    let mut seen = HashMap::new();
    for (key, _) in mime_types() {
        for ext in exts(key) {
            if let Some(other) = seen.insert(ext, key) {
                panic!("{ext} in {key} and {other}");
            }
        }
    }
}

#[test]
fn no_extension_in_two_ext_types() {
    let mut seen = HashMap::new();
    for (kind, list) in ext_types() {
        for ext in list {
            if let Some(other) = seen.insert(ext, kind) {
                panic!("{ext} in {kind} and {other}");
            }
        }
    }
}

#[test]
fn every_added_extension_has_its_ext_type() {
    let types = ext_types();
    let list = |name: &str| types.iter().find(|(k, _)| *k == name).unwrap().1.clone();
    for row in ADDED {
        let name = match row.kind {
            Kind::Image => "image",
            Kind::Audio => "audio",
            Kind::Video => "video",
        };
        for ext in exts(row.exts) {
            assert!(list(name).contains(&ext), "{ext} not under {name}");
        }
    }
    // WordPress's own lists come first, unchanged.
    for ((kind, wp), (ours_kind, ours)) in WORDPRESS_EXT_TYPES.iter().zip(&types) {
        assert_eq!(kind, ours_kind);
        assert_eq!(&ours[..wp.len()], *wp, "{kind}");
    }
    assert_eq!(types.len(), WORDPRESS_EXT_TYPES.len());
}

#[test]
fn left_out_extensions_absent() {
    assert!(!LEFT_OUT.is_empty());
    for (ext, reason) in LEFT_OUT {
        assert!(!reason.is_empty());
        for (key, _) in mime_types() {
            assert!(!exts(key).any(|e| e == *ext), "{ext} listed in {key}");
        }
    }
}

#[test]
fn image_mimes_to_exts_extends_wordpress() {
    let wordpress = [
        ("image/jpeg", "jpg"),
        ("image/png", "png"),
        ("image/gif", "gif"),
        ("image/bmp", "bmp"),
        ("image/tiff", "tif"),
        ("image/webp", "webp"),
        ("image/avif", "avif"),
        ("image/heic", "heic"),
        ("image/heif", "heic"),
        ("image/heic-sequence", "heic"),
        ("image/heif-sequence", "heic"),
    ];
    let map = image_mimes_to_exts();
    assert_eq!(&map[..wordpress.len()], &wordpress);
    for row in ADDED.iter().filter(|r| r.kind == Kind::Image) {
        let first = row.exts.split('|').next().unwrap();
        assert!(map.contains(&(row.mime, first)), "{}", row.mime);
    }
    assert_eq!(
        map.len(),
        wordpress.len() + ADDED.iter().filter(|r| r.kind == Kind::Image).count()
    );
}

#[test]
fn shared_mimes_keep_wordpress_default() {
    let types = mime_types();
    let first = |mime: &str| types.iter().find(|(_, m)| *m == mime).unwrap().0;
    assert_eq!(first("audio/mpeg"), "mp3|m4a|m4b");
    assert_eq!(first("video/mpeg"), "mpeg|mpg|mpe");
    assert_eq!(first("video/ogg"), "ogv");
}
