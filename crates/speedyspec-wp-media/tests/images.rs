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
        (head(&[0xFF, 0xD8, 0xFF, 0xE0]), "image/jpeg", 2, ImagePath::Fast),
        (head(b"\x89PNG\r\n\x1a\n"), "image/png", 3, ImagePath::Fast),
        (head(b"GIF89a"), "image/gif", 1, ImagePath::Fast),
        (head(b"GIF87a"), "image/gif", 1, ImagePath::Fast),
        (head(b"BM"), "image/bmp", 6, ImagePath::Fast),
        (head(b"II*\0"), "image/tiff", 7, ImagePath::Fast),
        (head(b"MM\0*"), "image/tiff", 8, ImagePath::Fast),
        (head(b"RIFF\x10\0\0\0WEBPVP8 "), "image/webp", 18, ImagePath::Fast),
        (ftyp(b"avif", &[b"mif1"]), "image/avif", 19, ImagePath::Fast),
        (ftyp(b"mif1", &[b"avif"]), "image/avif", 19, ImagePath::Fast),
        (ftyp(b"heic", &[b"mif1"]), "image/heif", 20, ImagePath::Fast),
        (head(b"\0\0\0\x0cjP  \r\n\x87\n"), "image/jp2", 10, ImagePath::Fast),
        (head(b"8BPS"), "image/psd", 5, ImagePath::Fast),
        (head(&[0, 0, 1, 0, 2, 0]), "image/vnd.microsoft.icon", 17, ImagePath::Fast),
        (head(&[0xFF, 0x0A]), "image/jxl", 0, ImagePath::Reader),
        (head(b"\0\0\0\x0cJXL \r\n\x87\n"), "image/jxl", 0, ImagePath::Reader),
        (head(b"qoif"), "image/qoi", 0, ImagePath::Reader),
        (head(&[0x0A, 5, 1, 8]), "image/x-pcx", 0, ImagePath::Reader),
        (head(b"P6\n64 48\n255\n"), "image/x-portable-pixmap", 0, ImagePath::Reader),
        (head(b"P5\n64 48\n255\n"), "image/x-portable-graymap", 0, ImagePath::Reader),
        (head(&[0x01, 0xDA, 0, 1]), "image/x-sgi", 0, ImagePath::Reader),
        (head(b"#?RADIANCE\n"), "image/vnd.radiance", 0, ImagePath::Reader),
    ];
    for (bytes, mime, imagetype, path) in cases {
        let t = sniff(&bytes).unwrap_or_else(|| panic!("{mime}: not sniffed"));
        assert_eq!((t.mime, t.imagetype, t.path), (mime, imagetype, path), "{mime}");
    }
}

#[test]
fn short_or_unknown_heads_are_none() {
    assert!(sniff(b"").is_none());
    assert!(sniff(&[0xFF, 0xD8]).is_none());
    assert!(sniff(&head(b"%PDF-1.7")).is_none());
    assert!(sniff(&ftyp(b"isom", &[b"mp41"])).is_none(), "an MP4 is not an image");
    assert!(sniff(&head(b"RIFF\x10\0\0\0WAVEfmt ")).is_none());
}

#[test]
fn fast_path_types() {
    let fast: Vec<&str> = speedyspec_wp_media::images::TYPES
        .iter()
        .filter(|t| t.path == ImagePath::Fast)
        .map(|t| t.mime)
        .collect();
    for mime in ["image/jpeg", "image/png", "image/gif", "image/bmp", "image/tiff", "image/webp", "image/avif", "image/heif", "image/jp2", "image/psd", "image/vnd.microsoft.icon"] {
        assert!(fast.contains(&mime), "{mime} should take the fast path");
    }
}
