//! Image types by content, the fast-path list, and WordPress's image rules.

use speedyspec_wp_media::images::{ImagePath, sniff};

fn head(bytes: &[u8]) -> Vec<u8> {
    let mut h = bytes.to_vec();
    h.resize(32, 0);
    h
}

fn ftyp(major: &[u8; 4], compatible: &[&[u8; 4]]) -> Vec<u8> {
    let mut h = vec![0, 0, 0, 0x1c];
    h.extend_from_slice(b"ftyp");
    h.extend_from_slice(major);
    h.extend_from_slice(&[0, 0, 0, 0]);
    for c in compatible {
        h.extend_from_slice(*c);
    }
    head(&h)
}

#[test]
fn sniffs_each_signature() {
    let cases: Vec<(Vec<u8>, &str, i64, ImagePath)> = vec![
        (
            head(&[0xFF, 0xD8, 0xFF, 0xE0]),
            "image/jpeg",
            2,
            ImagePath::Fast,
        ),
        (head(b"\x89PNG\r\n\x1a\n"), "image/png", 3, ImagePath::Fast),
        (head(b"GIF89a"), "image/gif", 1, ImagePath::Fast),
        (head(b"GIF87a"), "image/gif", 1, ImagePath::Fast),
        (head(b"BM"), "image/bmp", 6, ImagePath::Fast),
        (head(b"II*\0"), "image/tiff", 7, ImagePath::Fast),
        (head(b"MM\0*"), "image/tiff", 8, ImagePath::Fast),
        (
            head(b"RIFF\x10\0\0\0WEBPVP8 "),
            "image/webp",
            18,
            ImagePath::Fast,
        ),
        (ftyp(b"avif", &[b"mif1"]), "image/avif", 19, ImagePath::Fast),
        (ftyp(b"mif1", &[b"avif"]), "image/avif", 19, ImagePath::Fast),
        (ftyp(b"heic", &[b"mif1"]), "image/heif", 20, ImagePath::Fast),
        (
            head(b"\0\0\0\x0cjP  \r\n\x87\n"),
            "image/jp2",
            10,
            ImagePath::Fast,
        ),
        (head(b"8BPS"), "image/psd", 5, ImagePath::Fast),
        (
            head(&[0, 0, 1, 0, 2, 0]),
            "image/vnd.microsoft.icon",
            17,
            ImagePath::Fast,
        ),
        (head(&[0xFF, 0x0A]), "image/jxl", 0, ImagePath::Reader),
        (
            head(b"\0\0\0\x0cJXL \r\n\x87\n"),
            "image/jxl",
            0,
            ImagePath::Reader,
        ),
        (head(b"qoif"), "image/qoi", 0, ImagePath::Reader),
        (head(&[0x0A, 5, 1, 8]), "image/x-pcx", 0, ImagePath::Reader),
        (
            head(b"P6\n64 48\n255\n"),
            "image/x-portable-pixmap",
            0,
            ImagePath::Reader,
        ),
        (
            head(b"P5\n64 48\n255\n"),
            "image/x-portable-graymap",
            0,
            ImagePath::Reader,
        ),
        (
            head(&[0x01, 0xDA, 0, 1]),
            "image/x-sgi",
            0,
            ImagePath::Reader,
        ),
        (
            head(b"#?RADIANCE\n"),
            "image/vnd.radiance",
            0,
            ImagePath::Reader,
        ),
    ];
    for (bytes, mime, imagetype, path) in cases {
        let t = sniff(&bytes).unwrap_or_else(|| panic!("{mime}: not sniffed"));
        assert_eq!(
            (t.mime, t.imagetype, t.path),
            (mime, imagetype, path),
            "{mime}"
        );
    }
}

#[test]
fn short_or_unknown_heads_are_none() {
    assert!(sniff(b"").is_none());
    assert!(sniff(&[0xFF, 0xD8]).is_none());
    assert!(sniff(&head(b"%PDF-1.7")).is_none());
    assert!(
        sniff(&ftyp(b"isom", &[b"mp41"])).is_none(),
        "an MP4 is not an image"
    );
    assert!(sniff(&head(b"RIFF\x10\0\0\0WAVEfmt ")).is_none());
}

#[test]
fn fast_path_types() {
    let fast: Vec<&str> = speedyspec_wp_media::images::TYPES
        .iter()
        .filter(|t| t.path == ImagePath::Fast)
        .map(|t| t.mime)
        .collect();
    for mime in [
        "image/jpeg",
        "image/png",
        "image/gif",
        "image/bmp",
        "image/tiff",
        "image/webp",
        "image/avif",
        "image/heif",
        "image/jp2",
        "image/psd",
        "image/vnd.microsoft.icon",
    ] {
        assert!(fast.contains(&mime), "{mime} should take the fast path");
    }
}

use std::time::Duration;

use speedyspec_wp_media::images::{avif_info, reader_imagesize, webp_info};
use speedyspec_wp_media_contracts::{
    ContainerKind, ImageKind, ImageStream, MediaInfo, RawViews, Stream, Tree,
};

fn image(
    width: Option<u32>,
    height: Option<u32>,
    bits: Option<u8>,
    channels: Option<u8>,
) -> MediaInfo {
    MediaInfo {
        container: ContainerKind::Image(ImageKind::JpegXl),
        mime_type: Some("image/jxl".into()),
        duration: None::<Duration>,
        bitrate: None,
        file_size: Some(10),
        created_at: None,
        streams: vec![Stream::Image(ImageStream {
            kind: ImageKind::JpegXl,
            width,
            height,
            bit_depth: bits,
            channels,
            animated: false,
            lossless: None,
        })],
        tags: vec![],
        pictures: vec![],
        raw: RawViews::default(),
    }
}

fn keys(t: &Tree) -> Vec<String> {
    match t {
        Tree::Map(m) => m.iter().map(|(k, _)| k.clone()).collect(),
        _ => vec![],
    }
}

#[test]
fn reader_imagesize_shape() {
    let size =
        reader_imagesize(&image(Some(64), Some(48), Some(8), Some(3)), 0, "image/jxl").unwrap();
    assert_eq!(
        keys(&size),
        ["0", "1", "2", "3", "bits", "channels", "mime"]
    );
    assert_eq!(
        size.get("3"),
        Some(&Tree::Bytes(b"width=\"64\" height=\"48\"".to_vec()))
    );
    assert_eq!(size.get("mime"), Some(&Tree::Bytes(b"image/jxl".to_vec())));
    let plain = reader_imagesize(&image(Some(64), Some(48), None, None), 0, "image/jxl").unwrap();
    assert_eq!(keys(&plain), ["0", "1", "2", "3", "mime"]);
}

#[test]
fn reader_imagesize_none_without_dimensions() {
    assert!(reader_imagesize(&image(None, Some(48), None, None), 0, "image/jxl").is_none());
    assert!(reader_imagesize(&image(Some(0), Some(48), None, None), 0, "image/jxl").is_none());
}

fn webp(chunk: &[u8; 4], body: &[(usize, &[u8])]) -> Vec<u8> {
    let mut h = vec![0u8; 40];
    h[0..4].copy_from_slice(b"RIFF");
    h[8..12].copy_from_slice(b"WEBP");
    h[12..16].copy_from_slice(chunk);
    for (at, bytes) in body {
        h[*at..*at + bytes.len()].copy_from_slice(bytes);
    }
    h
}

fn triple(t: &Tree) -> (Tree, Tree, Tree) {
    (
        t.get("width").unwrap().clone(),
        t.get("height").unwrap().clone(),
        t.get("type").unwrap().clone(),
    )
}

#[test]
fn webp_info_lossy_lossless_vp8x() {
    let lossy = webp(b"VP8 ", &[(26, &[0x40, 0x00, 0x30, 0x00])]);
    assert_eq!(
        triple(&webp_info(&lossy)),
        (Tree::Int(64), Tree::Int(48), Tree::Bytes(b"lossy".to_vec()))
    );
    // VP8L: 14-bit width-1 then height-1, packed little-endian after the 0x2F signature at byte 20.
    let (w, h) = (64u32 - 1, 48u32 - 1);
    let bits = w | (h << 14);
    let lossless = webp(b"VP8L", &[(21, &bits.to_le_bytes())]);
    assert_eq!(
        triple(&webp_info(&lossless)),
        (
            Tree::Int(64),
            Tree::Int(48),
            Tree::Bytes(b"lossless".to_vec())
        )
    );
    let vp8x = webp(b"VP8X", &[(24, &[63, 0, 0]), (27, &[47, 0, 0])]);
    assert_eq!(
        triple(&webp_info(&vp8x)),
        (
            Tree::Int(64),
            Tree::Int(48),
            Tree::Bytes(b"animated-alpha".to_vec())
        )
    );
}

#[test]
fn webp_info_short_file_is_all_false() {
    let f = Tree::Bool(false);
    assert_eq!(
        triple(&webp_info(&[0u8; 39])),
        (f.clone(), f.clone(), f.clone())
    );
    assert_eq!(
        triple(&webp_info(&webp(b"ABCD", &[]))),
        (f.clone(), f.clone(), f)
    );
}

#[test]
fn avif_info_fields() {
    let info = avif_info(&image(Some(64), Some(48), Some(10), Some(4)));
    assert_eq!(
        keys(&info),
        ["width", "height", "bit_depth", "num_channels"]
    );
    assert_eq!(info.get("num_channels"), Some(&Tree::Int(4)));
    let unknown = avif_info(&image(Some(64), Some(48), None, None));
    assert_eq!(unknown.get("bit_depth"), Some(&Tree::Bool(false)));
}
