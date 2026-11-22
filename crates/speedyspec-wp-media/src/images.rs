//! Image types by content, which of them take the fast path, and WordPress's image rules over the reader.
//!
//! The fast path is PHP 8.5's own `getimagesize()`: the types it reads correctly (`compat/media/recorded-images.json`)
//! go to it, so their arrays are exactly WordPress's. Every other image type FFmpeg reads goes through the media
//! reader. Types and MIME types follow `exif_imagetype()` and `image_type_to_mime_type()`, so a file is typed by its
//! content, whatever its name.

use speedyspec_wp_media_contracts::{MediaInfo, Stream, Tree};

/// How a type's size is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImagePath {
    /// PHP's `getimagesize()`.
    Fast,
    /// The media reader (FFmpeg in an isolated child).
    Reader,
}

/// An image type: its MIME type, PHP's `IMAGETYPE_*` value (0 when PHP has none) and its path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageType {
    pub mime: &'static str,
    pub imagetype: i64,
    pub path: ImagePath,
}

const fn t(mime: &'static str, imagetype: i64, path: ImagePath) -> ImageType {
    ImageType {
        mime,
        imagetype,
        path,
    }
}

pub const JPEG: ImageType = t("image/jpeg", 2, ImagePath::Fast);
pub const PNG: ImageType = t("image/png", 3, ImagePath::Fast);
pub const GIF: ImageType = t("image/gif", 1, ImagePath::Fast);
pub const BMP: ImageType = t("image/bmp", 6, ImagePath::Fast);
pub const TIFF_II: ImageType = t("image/tiff", 7, ImagePath::Fast);
pub const TIFF_MM: ImageType = t("image/tiff", 8, ImagePath::Fast);
pub const WEBP: ImageType = t("image/webp", 18, ImagePath::Fast);
pub const AVIF: ImageType = t("image/avif", 19, ImagePath::Fast);
pub const HEIF: ImageType = t("image/heif", 20, ImagePath::Fast);
pub const JP2: ImageType = t("image/jp2", 10, ImagePath::Fast);
pub const PSD: ImageType = t("image/psd", 5, ImagePath::Fast);
pub const ICO: ImageType = t("image/vnd.microsoft.icon", 17, ImagePath::Fast);
pub const JXL: ImageType = t("image/jxl", 0, ImagePath::Reader);
pub const QOI: ImageType = t("image/qoi", 0, ImagePath::Reader);
pub const PCX: ImageType = t("image/x-pcx", 0, ImagePath::Reader);
pub const PPM: ImageType = t("image/x-portable-pixmap", 0, ImagePath::Reader);
pub const PGM: ImageType = t("image/x-portable-graymap", 0, ImagePath::Reader);
pub const SGI: ImageType = t("image/x-sgi", 0, ImagePath::Reader);
pub const HDR: ImageType = t("image/vnd.radiance", 0, ImagePath::Reader);

/// Every type `sniff` recognizes.
pub const TYPES: &[ImageType] = &[
    JPEG, PNG, GIF, BMP, TIFF_II, TIFF_MM, WEBP, AVIF, HEIF, JP2, PSD, ICO, JXL, QOI, PCX, PPM,
    PGM, SGI, HDR,
];

const HEIF_BRANDS: &[&[u8]] = &[
    b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"mif1", b"msf1",
];

/// The image type of a file from its first bytes (32 are enough), or `None` for anything else.
pub fn sniff(head: &[u8]) -> Option<ImageType> {
    let starts = |sig: &[u8]| head.starts_with(sig);
    if starts(&[0xFF, 0xD8, 0xFF]) {
        return Some(JPEG);
    }
    if starts(b"\x89PNG\r\n\x1a\n") {
        return Some(PNG);
    }
    if starts(b"GIF87a") || starts(b"GIF89a") {
        return Some(GIF);
    }
    if starts(b"BM") && head.len() >= 14 {
        return Some(BMP);
    }
    if starts(b"II*\0") {
        return Some(TIFF_II);
    }
    if starts(b"MM\0*") {
        return Some(TIFF_MM);
    }
    if starts(b"RIFF") && head.get(8..12) == Some(b"WEBP") {
        return Some(WEBP);
    }
    if head.get(4..8) == Some(b"ftyp") {
        let major = head.get(8..12)?;
        let box_len = u32::from_be_bytes(head[0..4].try_into().ok()?) as usize;
        let compatible = head
            .get(16..box_len.min(head.len()))
            .unwrap_or(&[])
            .as_chunks::<4>()
            .0;
        if major == b"avif"
            || major == b"avis"
            || (major == b"mif1" && compatible.contains(b"avif"))
        {
            return Some(AVIF);
        }
        if HEIF_BRANDS.contains(&major) {
            return Some(HEIF);
        }
        return None;
    }
    if starts(b"\0\0\0\x0cjP  \r\n\x87\n") {
        return Some(JP2);
    }
    if starts(b"8BPS") {
        return Some(PSD);
    }
    if starts(&[0, 0, 1, 0]) && head.get(4).is_some_and(|&n| n > 0) {
        return Some(ICO);
    }
    if starts(&[0xFF, 0x0A]) || starts(b"\0\0\0\x0cJXL \r\n\x87\n") {
        return Some(JXL);
    }
    if starts(b"qoif") {
        return Some(QOI);
    }
    if head.len() >= 3 && head[0] == 0x0A && matches!(head[1], 0 | 2 | 3 | 4 | 5) && head[2] == 1 {
        return Some(PCX);
    }
    if head.len() >= 3 && (starts(b"P6") || starts(b"P5")) && head[2].is_ascii_whitespace() {
        return Some(if head[1] == b'6' { PPM } else { PGM });
    }
    if starts(&[0x01, 0xDA]) {
        return Some(SGI);
    }
    if starts(b"#?RADIANCE") || starts(b"#?RGBE") {
        return Some(HDR);
    }
    None
}

fn b(s: &str) -> Tree {
    Tree::Bytes(s.as_bytes().to_vec())
}

/// `wp_getimagesize()`'s array for a reader-path type, from the first image (or video) stream:
/// `[w, h, imagetype, 'width="w" height="h"', 'bits' => …, 'channels' => …, 'mime' => mime]`, bits and channels only
/// when known. `None` when a dimension is unknown or zero.
pub fn reader_imagesize(info: &MediaInfo, imagetype: i64, mime: &str) -> Option<Tree> {
    let (w, h, bits, channels) = info.streams.iter().find_map(|s| match s {
        Stream::Image(i) => Some((i.width?, i.height?, i.bit_depth, i.channels)),
        Stream::Video(v) => Some((v.width?, v.height?, None, None)),
        _ => None,
    })?;
    if w == 0 || h == 0 {
        return None;
    }
    let mut m = vec![
        ("0".to_owned(), Tree::Int(w.into())),
        ("1".to_owned(), Tree::Int(h.into())),
        ("2".to_owned(), Tree::Int(imagetype)),
        ("3".to_owned(), b(&format!("width=\"{w}\" height=\"{h}\""))),
    ];
    if let Some(bits) = bits {
        m.push(("bits".into(), Tree::Int(bits.into())));
    }
    if let Some(channels) = channels {
        m.push(("channels".into(), Tree::Int(channels.into())));
    }
    m.push(("mime".into(), b(mime)));
    Some(Tree::Map(m))
}

fn webp_result(width: Option<i64>, height: Option<i64>, kind: Option<&str>) -> Tree {
    let or_false = |v: Option<Tree>| v.unwrap_or(Tree::Bool(false));
    Tree::Map(vec![
        ("width".into(), or_false(width.map(Tree::Int))),
        ("height".into(), or_false(height.map(Tree::Int))),
        ("type".into(), or_false(kind.map(b))),
    ])
}

/// WordPress 7.1.5's `wp_get_webp_info()` after its MIME check, over the file's first 40 bytes.
pub fn webp_info(head: &[u8]) -> Tree {
    if head.len() < 40 {
        return webp_result(None, None, None);
    }
    let le16 = |at: usize| i64::from(u16::from_le_bytes([head[at], head[at + 1]]));
    let le24 = |at: usize| {
        i64::from(u32::from_le_bytes([
            head[at],
            head[at + 1],
            head[at + 2],
            0,
        ]))
    };
    match &head[12..16] {
        b"VP8 " => webp_result(
            Some(le16(26) & 0x3FFF),
            Some(le16(28) & 0x3FFF),
            Some("lossy"),
        ),
        b"VP8L" => {
            let p = |i: usize| i64::from(head[21 + i]);
            let width = (p(0) | ((p(1) & 0x3F) << 8)) + 1;
            let height = (((p(1) & 0xC0) >> 6) | (p(2) << 2) | ((p(3) & 0x03) << 10)) + 1;
            webp_result(Some(width), Some(height), Some("lossless"))
        }
        b"VP8X" => webp_result(
            Some((le24(24) & 0xFF_FFFF) + 1),
            Some((le24(27) & 0xFF_FFFF) + 1),
            Some("animated-alpha"),
        ),
        _ => webp_result(None, None, None),
    }
}

/// WordPress's `wp_get_avif_info()` result from the reader: width, height, bit depth and channel count (4 with an
/// alpha channel), each `false` when unknown.
pub fn avif_info(info: &MediaInfo) -> Tree {
    let found = info.streams.iter().find_map(|s| match s {
        Stream::Image(i) => Some((i.width, i.height, i.bit_depth, i.channels)),
        Stream::Video(v) => Some((v.width, v.height, None, None)),
        _ => None,
    });
    let (w, h, bits, channels) = found.unwrap_or((None, None, None, None));
    let int = |v: Option<i64>| v.map(Tree::Int).unwrap_or(Tree::Bool(false));
    Tree::Map(vec![
        ("width".into(), int(w.map(i64::from))),
        ("height".into(), int(h.map(i64::from))),
        ("bit_depth".into(), int(bits.map(i64::from))),
        ("num_channels".into(), int(channels.map(i64::from))),
    ])
}
