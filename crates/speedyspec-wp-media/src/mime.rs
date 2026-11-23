//! WordPress's MIME tables, widened to every audio, video and image format the media reader reads that has a usual
//! extension and MIME type.
//!
//! WordPress 7.1.5's own lists are kept verbatim and in order ([`WORDPRESS_MIME_TYPES`], [`WORDPRESS_EXT_TYPES`]);
//! each added row ([`ADDED`]) follows the last WordPress row of its kind and is proven by a corpus file the reader
//! reads as that kind. Extensions that clash with a non-media type, or that the build can't size, are left out
//! ([`LEFT_OUT`]). [`content_mime`] is the content check `wp_check_filetype_and_ext()` makes.

use std::path::Path;

use speedyspec_wp_media_contracts::{
    ContentSniffer, MediaReader, MediaSource, ProbeLimits, Stream,
};

/// Which kind of media a row lists, and so where it goes in the lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Image,
    Audio,
    Video,
}

/// A row the widened list adds: WordPress's `ext|ext` key, its MIME type, its kind, and the corpus file (under
/// `compat/media/`) that proves the reader reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddedRow {
    pub exts: &'static str,
    pub mime: &'static str,
    pub kind: Kind,
    pub corpus: &'static str,
}

const fn row(exts: &'static str, mime: &'static str, kind: Kind, corpus: &'static str) -> AddedRow {
    AddedRow {
        exts,
        mime,
        kind,
        corpus,
    }
}

/// WordPress 7.1.5's `wp_get_mime_types()` before its filter, in order.
pub const WORDPRESS_MIME_TYPES: &[(&str, &str)] = &[
    ("jpg|jpeg|jpe", "image/jpeg"),
    ("gif", "image/gif"),
    ("png", "image/png"),
    ("bmp", "image/bmp"),
    ("tiff|tif", "image/tiff"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("ico", "image/x-icon"),
    ("heic", "image/heic"),
    ("heif", "image/heif"),
    ("heics", "image/heic-sequence"),
    ("heifs", "image/heif-sequence"),
    ("asf|asx", "video/x-ms-asf"),
    ("wmv", "video/x-ms-wmv"),
    ("wmx", "video/x-ms-wmx"),
    ("wm", "video/x-ms-wm"),
    ("avi", "video/avi"),
    ("divx", "video/divx"),
    ("flv", "video/x-flv"),
    ("mov|qt", "video/quicktime"),
    ("mpeg|mpg|mpe", "video/mpeg"),
    ("mp4|m4v", "video/mp4"),
    ("ogv", "video/ogg"),
    ("webm", "video/webm"),
    ("mkv", "video/x-matroska"),
    ("3gp|3gpp", "video/3gpp"),
    ("3g2|3gp2", "video/3gpp2"),
    ("txt|asc|c|cc|h|srt", "text/plain"),
    ("csv", "text/csv"),
    ("tsv", "text/tab-separated-values"),
    ("ics", "text/calendar"),
    ("rtx", "text/richtext"),
    ("css", "text/css"),
    ("htm|html", "text/html"),
    ("vtt", "text/vtt"),
    ("dfxp", "application/ttaf+xml"),
    ("mp3|m4a|m4b", "audio/mpeg"),
    ("aac", "audio/aac"),
    ("ra|ram", "audio/x-realaudio"),
    ("wav|x-wav", "audio/wav"),
    ("ogg|oga", "audio/ogg"),
    ("flac", "audio/flac"),
    ("mid|midi", "audio/midi"),
    ("wma", "audio/x-ms-wma"),
    ("wax", "audio/x-ms-wax"),
    ("mka", "audio/x-matroska"),
    ("rtf", "application/rtf"),
    ("js", "application/javascript"),
    ("pdf", "application/pdf"),
    ("swf", "application/x-shockwave-flash"),
    ("class", "application/java"),
    ("tar", "application/x-tar"),
    ("zip", "application/zip"),
    ("gz|gzip", "application/x-gzip"),
    ("rar", "application/rar"),
    ("7z", "application/x-7z-compressed"),
    ("exe", "application/x-msdownload"),
    ("psd", "application/octet-stream"),
    ("xcf", "application/octet-stream"),
    ("doc", "application/msword"),
    ("pot|pps|ppt", "application/vnd.ms-powerpoint"),
    ("wri", "application/vnd.ms-write"),
    ("xla|xls|xlt|xlw", "application/vnd.ms-excel"),
    ("mdb", "application/vnd.ms-access"),
    ("mpp", "application/vnd.ms-project"),
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    ("docm", "application/vnd.ms-word.document.macroEnabled.12"),
    (
        "dotx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.template",
    ),
    ("dotm", "application/vnd.ms-word.template.macroEnabled.12"),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    ("xlsm", "application/vnd.ms-excel.sheet.macroEnabled.12"),
    (
        "xlsb",
        "application/vnd.ms-excel.sheet.binary.macroEnabled.12",
    ),
    (
        "xltx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.template",
    ),
    ("xltm", "application/vnd.ms-excel.template.macroEnabled.12"),
    ("xlam", "application/vnd.ms-excel.addin.macroEnabled.12"),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    (
        "pptm",
        "application/vnd.ms-powerpoint.presentation.macroEnabled.12",
    ),
    (
        "ppsx",
        "application/vnd.openxmlformats-officedocument.presentationml.slideshow",
    ),
    (
        "ppsm",
        "application/vnd.ms-powerpoint.slideshow.macroEnabled.12",
    ),
    (
        "potx",
        "application/vnd.openxmlformats-officedocument.presentationml.template",
    ),
    (
        "potm",
        "application/vnd.ms-powerpoint.template.macroEnabled.12",
    ),
    (
        "ppam",
        "application/vnd.ms-powerpoint.addin.macroEnabled.12",
    ),
    (
        "sldx",
        "application/vnd.openxmlformats-officedocument.presentationml.slide",
    ),
    (
        "sldm",
        "application/vnd.ms-powerpoint.slide.macroEnabled.12",
    ),
    ("onetoc|onetoc2|onetmp|onepkg", "application/onenote"),
    ("oxps", "application/oxps"),
    ("xps", "application/vnd.ms-xpsdocument"),
    ("odt", "application/vnd.oasis.opendocument.text"),
    ("odp", "application/vnd.oasis.opendocument.presentation"),
    ("ods", "application/vnd.oasis.opendocument.spreadsheet"),
    ("odg", "application/vnd.oasis.opendocument.graphics"),
    ("odc", "application/vnd.oasis.opendocument.chart"),
    ("odb", "application/vnd.oasis.opendocument.database"),
    ("odf", "application/vnd.oasis.opendocument.formula"),
    ("wp|wpd", "application/wordperfect"),
    ("key", "application/vnd.apple.keynote"),
    ("numbers", "application/vnd.apple.numbers"),
    ("pages", "application/vnd.apple.pages"),
];

/// WordPress 7.1.5's `wp_get_ext_types()` before its filter, in order.
pub const WORDPRESS_EXT_TYPES: &[(&str, &[&str])] = &[
    (
        "image",
        &[
            "jpg", "jpeg", "jpe", "gif", "png", "bmp", "tif", "tiff", "ico", "heic", "heif",
            "webp", "avif",
        ],
    ),
    (
        "audio",
        &[
            "aac", "ac3", "aif", "aiff", "flac", "m3a", "m4a", "m4b", "mka", "mp1", "mp2", "mp3",
            "ogg", "oga", "ram", "wav", "wma",
        ],
    ),
    (
        "video",
        &[
            "3g2", "3gp", "3gpp", "asf", "avi", "divx", "dv", "flv", "m4v", "mkv", "mov", "mp4",
            "mpeg", "mpg", "mpv", "ogm", "ogv", "qt", "rm", "vob", "wmv",
        ],
    ),
    (
        "document",
        &[
            "doc", "docx", "docm", "dotm", "odt", "pages", "pdf", "xps", "oxps", "rtf", "wp",
            "wpd", "psd", "xcf",
        ],
    ),
    (
        "spreadsheet",
        &["numbers", "ods", "xls", "xlsx", "xlsm", "xlsb"],
    ),
    (
        "interactive",
        &[
            "swf", "key", "ppt", "pptx", "pptm", "pps", "ppsx", "ppsm", "sldx", "sldm", "odp",
        ],
    ),
    ("text", &["asc", "csv", "tsv", "txt"]),
    (
        "archive",
        &[
            "bz2", "cab", "dmg", "gz", "rar", "sea", "sit", "sqx", "tar", "tgz", "zip", "7z",
        ],
    ),
    ("code", &["css", "htm", "html", "php", "js"]),
];

/// The rows the widened list adds, by kind in list order.
pub const ADDED: &[AddedRow] = &[
    row("jp2", "image/jp2", Kind::Image, "images/photo.jp2"),
    row("jxl", "image/jxl", Kind::Image, "images/photo.jxl"),
    row("qoi", "image/qoi", Kind::Image, "images/photo.qoi"),
    row("pcx", "image/x-pcx", Kind::Image, "images/photo.pcx"),
    row(
        "ppm",
        "image/x-portable-pixmap",
        Kind::Image,
        "images/photo.ppm",
    ),
    row(
        "pgm",
        "image/x-portable-graymap",
        Kind::Image,
        "images/photo.pgm",
    ),
    row("sgi", "image/x-sgi", Kind::Image, "images/photo.sgi"),
    row("hdr", "image/vnd.radiance", Kind::Image, "images/photo.hdr"),
    row("m2ts|mts", "video/mp2t", Kind::Video, "uploads/bars.m2ts"),
    row("mxf", "application/mxf", Kind::Video, "uploads/bars.mxf"),
    row("dv", "video/dv", Kind::Video, "uploads/bars.dv"),
    row("vob", "video/mpeg", Kind::Video, "uploads/bars.vob"),
    row("m2v|mpv", "video/mpeg", Kind::Video, "uploads/bars.m2v"),
    row(
        "rm|rmvb",
        "application/vnd.rn-realmedia",
        Kind::Video,
        "uploads/bars.rm",
    ),
    row("ogm", "video/ogg", Kind::Video, "uploads/bars.ogm"),
    row(
        "aif|aiff|aifc",
        "audio/aiff",
        Kind::Audio,
        "corpus/tone.aiff",
    ),
    row("ac3", "audio/ac3", Kind::Audio, "corpus/tone.ac3"),
    row("eac3|ec3", "audio/eac3", Kind::Audio, "uploads/tone.eac3"),
    row("opus", "audio/opus", Kind::Audio, "corpus/tone.opus"),
    row("caf", "audio/x-caf", Kind::Audio, "uploads/tone.caf"),
    row("wv", "audio/x-wavpack", Kind::Audio, "uploads/tone.wv"),
    row("tta", "audio/x-tta", Kind::Audio, "uploads/tone.tta"),
    row("au|snd", "audio/basic", Kind::Audio, "uploads/tone.au"),
    row("mp1|mp2|m2a", "audio/mpeg", Kind::Audio, "uploads/tone.mp2"),
    row("w64", "audio/x-w64", Kind::Audio, "uploads/tone.w64"),
    row("voc", "audio/x-voc", Kind::Audio, "uploads/tone.voc"),
];

/// Extensions left out of the list, and why.
pub const LEFT_OUT: &[(&str, &str)] = &[
    ("ts", "also TypeScript (and Qt translation) source"),
    ("dts", "also Device Tree Source"),
    (
        "tga",
        "no magic number to type it by, and no decoder in the build to size it",
    ),
    (
        "pbm",
        "its decoder is not in the build, so it can't be sized",
    ),
    ("rgb", "a generic name many raw formats share"),
];

/// The last WordPress key of each kind, after which that kind's added rows go.
const ANCHORS: [(Kind, &str); 3] = [
    (Kind::Image, "heifs"),
    (Kind::Video, "3g2|3gp2"),
    (Kind::Audio, "mka"),
];

/// `wp_get_mime_types()`'s list before its filter: WordPress's, with each added row after its kind's anchor.
pub fn mime_types() -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::with_capacity(WORDPRESS_MIME_TYPES.len() + ADDED.len());
    for &(key, mime) in WORDPRESS_MIME_TYPES {
        out.push((key, mime));
        if let Some((kind, _)) = ANCHORS.iter().find(|(_, anchor)| *anchor == key) {
            out.extend(
                ADDED
                    .iter()
                    .filter(|r| r.kind == *kind)
                    .map(|r| (r.exts, r.mime)),
            );
        }
    }
    out
}

/// `wp_get_ext_types()`'s list before its filter: WordPress's, with each added extension WordPress lacks appended
/// to `image`, `audio` or `video`.
pub fn ext_types() -> Vec<(&'static str, Vec<&'static str>)> {
    let known: Vec<&str> = WORDPRESS_EXT_TYPES
        .iter()
        .flat_map(|(_, list)| list.iter().copied())
        .collect();
    WORDPRESS_EXT_TYPES
        .iter()
        .map(|&(name, list)| {
            let kind = match name {
                "image" => Some(Kind::Image),
                "audio" => Some(Kind::Audio),
                "video" => Some(Kind::Video),
                _ => None,
            };
            let mut list = list.to_vec();
            for row in ADDED.iter().filter(|r| Some(r.kind) == kind) {
                list.extend(row.exts.split('|').filter(|e| !known.contains(e)));
            }
            (name, list)
        })
        .collect()
}

/// The `getimagesize_mimes_to_exts` filter's default: WordPress's eleven, then each added image type's MIME and
/// first extension.
pub fn image_mimes_to_exts() -> Vec<(&'static str, &'static str)> {
    let mut out = vec![
        ("image/jpeg", "jpg"),
        ("image/png", "png"),
        ("image/gif", "gif"),
        ("image/bmp", "bmp"),
        ("image/tiff", "tif"),
        ("image/webp", "webp"),
        ("image/avif", "avif"),
        // WordPress's own choice: every HEIC and HEIF type takes the extension HEIC files commonly carry.
        ("image/heic", "heic"),
        ("image/heif", "heic"),
        ("image/heic-sequence", "heic"),
        ("image/heif-sequence", "heic"),
    ];
    for row in ADDED.iter().filter(|r| r.kind == Kind::Image) {
        out.push((row.mime, row.exts.split('|').next().unwrap_or(row.exts)));
    }
    out
}

/// The MIME type `wp_check_filetype_and_ext()` compares with `claimed` (the type the file's name gives it).
///
/// It is fileinfo's answer, except in one case. When both the claim and fileinfo are audio or video but of different
/// kinds (an audio-only Matroska that fileinfo calls `video/x-matroska`), the media reader decides. If it reads a
/// stream of the claimed kind, the answer is that kind with the reader's (or the claim's) subtype. The reader can
/// only turn WordPress's prefix mismatch into an accept: it is never consulted for content fileinfo calls anything
/// else, so documents and files that only pretend to be media are judged exactly as WordPress judges them.
pub fn content_mime(
    path: &Path,
    claimed: &str,
    fileinfo: &dyn ContentSniffer,
    reader: &dyn MediaReader,
    limits: &ProbeLimits,
) -> Option<String> {
    let found = fileinfo.sniff(path)?;
    let claimed_kind = media_kind(claimed);
    let found_kind = media_kind(&found);
    let (Some(claimed_kind), Some(found_kind)) = (claimed_kind, found_kind) else {
        return Some(found);
    };
    if claimed_kind == found_kind {
        return Some(found);
    }
    let Ok(info) = reader.read(MediaSource::Path(path.to_path_buf()), limits) else {
        return Some(found);
    };
    let has_video = info.streams.iter().any(|s| matches!(s, Stream::Video(_)));
    let has_audio = info.streams.iter().any(|s| matches!(s, Stream::Audio(_)));
    let confirmed = match claimed_kind {
        Kind::Video => has_video,
        Kind::Audio => has_audio && !has_video,
        Kind::Image => false,
    };
    if !confirmed {
        return Some(found);
    }
    let subtype = info
        .mime_type
        .as_deref()
        .and_then(|m| m.split_once('/'))
        .or_else(|| claimed.split_once('/'))
        .map_or("", |(_, subtype)| subtype);
    let top = if claimed_kind == Kind::Video {
        "video"
    } else {
        "audio"
    };
    Some(format!("{top}/{subtype}"))
}

/// `audio/…` or `video/…`, by the MIME type's top-level type.
fn media_kind(mime: &str) -> Option<Kind> {
    match mime.split_once('/').map(|(top, _)| top) {
        Some("audio") => Some(Kind::Audio),
        Some("video") => Some(Kind::Video),
        _ => None,
    }
}
