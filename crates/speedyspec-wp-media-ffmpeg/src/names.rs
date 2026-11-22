//! FFmpeg's names → neutral ones: containers, MIME types, codecs, tag keys, tag systems, pixel formats.

use speedyspec_wp_media_contracts::{Codec, ContainerKind, ImageKind, TagKey, TagSystem};

/// The container FFmpeg's `format_name` (and, for the ISO family, the `major_brand` tag) names. `codecs` decides
/// WebM within `matroska,webm`.
pub fn container(format: &str, brand: Option<&str>, codecs: &[Codec]) -> ContainerKind {
    match format {
        "mov,mp4,m4a,3gp,3g2,mj2" => {
            let brand = brand.unwrap_or("").trim_end();
            match brand {
                "qt" => ContainerKind::QuickTime,
                b if b.starts_with("3gp") || b.starts_with("3g2") => ContainerKind::ThreeGp,
                "avif" | "avis" => ContainerKind::Image(ImageKind::Avif),
                "heic" | "heix" | "mif1" | "msf1" | "heim" | "heis" | "hevc" | "hevx" => {
                    ContainerKind::Image(ImageKind::Heif)
                }
                _ => ContainerKind::Mp4,
            }
        }
        "matroska,webm" => {
            let webm = !codecs.is_empty()
                && codecs.iter().all(|c| {
                    matches!(
                        c,
                        Codec::Vp8 | Codec::Vp9 | Codec::Av1 | Codec::Vorbis | Codec::Opus
                    )
                });
            if webm {
                ContainerKind::WebM
            } else {
                ContainerKind::Matroska
            }
        }
        "mp3" => ContainerKind::Mp3,
        "aac" => ContainerKind::Aac,
        "flac" => ContainerKind::Flac,
        "ogg" => ContainerKind::Ogg,
        "wav" | "w64" => ContainerKind::Wav,
        "aiff" => ContainerKind::Aiff,
        "asf" => ContainerKind::Asf,
        "avi" => ContainerKind::Avi,
        "flv" | "live_flv" => ContainerKind::Flv,
        "mpeg" => ContainerKind::MpegPs,
        "mpegts" | "mpegtsraw" => ContainerKind::MpegTs,
        "ac3" | "eac3" => ContainerKind::Ac3,
        "dts" | "dtshd" => ContainerKind::Dts,
        "rm" => ContainerKind::RealMedia,
        "webp_anim" => ContainerKind::Image(ImageKind::Webp),
        "jpegxl_anim" | "jpegxl_pipe" => ContainerKind::Image(ImageKind::JpegXl),
        "ico" => ContainerKind::Image(ImageKind::Ico),
        "gif" => ContainerKind::Image(ImageKind::Gif),
        "apng" => ContainerKind::Image(ImageKind::Png),
        other => match other.strip_suffix("_pipe") {
            Some(kind) => ContainerKind::Image(image_kind(kind)),
            None => ContainerKind::Other(other.to_owned()),
        },
    }
}

fn image_kind(pipe: &str) -> ImageKind {
    match pipe {
        "jpeg" | "jpegls" => ImageKind::Jpeg,
        "png" => ImageKind::Png,
        "gif" => ImageKind::Gif,
        "webp" => ImageKind::Webp,
        "bmp" => ImageKind::Bmp,
        "tiff" => ImageKind::Tiff,
        other => ImageKind::Other(other.to_owned()),
    }
}

/// The IANA (or customary) MIME type of a container, by whether it has video.
pub fn mime(container: &ContainerKind, has_video: bool, has_theora: bool) -> Option<String> {
    let m = match container {
        ContainerKind::Mp4 => {
            if has_video {
                "video/mp4"
            } else {
                "audio/mp4"
            }
        }
        ContainerKind::QuickTime => "video/quicktime",
        ContainerKind::ThreeGp => "video/3gpp",
        ContainerKind::Matroska => {
            if has_video {
                "video/x-matroska"
            } else {
                "audio/x-matroska"
            }
        }
        ContainerKind::WebM => {
            if has_video {
                "video/webm"
            } else {
                "audio/webm"
            }
        }
        ContainerKind::Ogg => {
            if has_theora {
                "video/ogg"
            } else {
                "audio/ogg"
            }
        }
        ContainerKind::Asf => {
            if has_video {
                "video/x-ms-asf"
            } else {
                "audio/x-ms-wma"
            }
        }
        ContainerKind::Wav => "audio/wav",
        ContainerKind::Avi => "video/x-msvideo",
        ContainerKind::Aiff => "audio/aiff",
        ContainerKind::Flv => "video/x-flv",
        ContainerKind::MpegPs => "video/mpeg",
        ContainerKind::MpegTs => "video/mp2t",
        ContainerKind::Mp3 => "audio/mpeg",
        ContainerKind::Aac => "audio/aac",
        ContainerKind::Flac => "audio/flac",
        ContainerKind::Ac3 => "audio/ac3",
        ContainerKind::Dts => "audio/vnd.dts",
        ContainerKind::RealMedia => {
            if has_video {
                "application/vnd.rn-realmedia"
            } else {
                "audio/x-pn-realaudio"
            }
        }
        ContainerKind::Midi => "audio/midi",
        ContainerKind::Image(kind) => match kind {
            ImageKind::Jpeg => "image/jpeg",
            ImageKind::Png => "image/png",
            ImageKind::Gif => "image/gif",
            ImageKind::Webp => "image/webp",
            ImageKind::Bmp => "image/bmp",
            ImageKind::Tiff => "image/tiff",
            ImageKind::Avif => "image/avif",
            ImageKind::Heif => "image/heif",
            ImageKind::JpegXl => "image/jxl",
            ImageKind::Ico => "image/vnd.microsoft.icon",
            _ => return None,
        },
        _ => return None,
    };
    Some(m.to_owned())
}

/// The codec FFmpeg's `codec_name` names.
pub fn codec(name: &str) -> Codec {
    if let Some(pcm) = name.strip_prefix("pcm_") {
        let float = pcm.starts_with('f');
        let digits: String = pcm
            .chars()
            .skip(1)
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(bits) = digits.parse() {
            return Codec::Pcm { bits, float };
        }
    }
    match name {
        "mp3" | "mp3float" => Codec::Mp3,
        "mp2" | "mp1" => Codec::Mp2,
        "aac" | "aac_latm" => Codec::Aac,
        "flac" => Codec::Flac,
        "vorbis" => Codec::Vorbis,
        "opus" => Codec::Opus,
        "alac" => Codec::Alac,
        "wmav1" => Codec::WmaV1,
        "wmav2" => Codec::WmaV2,
        "wmapro" => Codec::WmaPro,
        "wmalossless" => Codec::WmaLossless,
        "ac3" => Codec::Ac3,
        "eac3" => Codec::Eac3,
        "dts" => Codec::Dts,
        "truehd" => Codec::TrueHd,
        "cook" | "ra_144" | "ra_288" | "sipr" | "atrac3" => Codec::RealAudio(name.to_owned()),
        "wavpack" => Codec::WavPack,
        "ape" => Codec::Ape,
        "h264" => Codec::H264,
        "hevc" => Codec::Hevc,
        "av1" => Codec::Av1,
        "vp8" => Codec::Vp8,
        "vp9" => Codec::Vp9,
        "mpeg1video" => Codec::Mpeg1Video,
        "mpeg2video" => Codec::Mpeg2Video,
        "mpeg4" => Codec::Mpeg4Part2,
        "wmv1" | "wmv2" | "wmv3" | "vc1" => Codec::Wmv(name.to_owned()),
        "theora" => Codec::Theora,
        "rv10" | "rv20" | "rv30" | "rv40" | "rv60" => Codec::RealVideo(name.to_owned()),
        "mjpeg" | "jpegls" => Codec::Jpeg,
        "png" | "apng" => Codec::Png,
        "gif" => Codec::Gif,
        "webp" => Codec::Webp,
        "bmp" => Codec::Bmp,
        "tiff" => Codec::Tiff,
        other => Codec::Other(other.to_owned()),
    }
}

/// Whether `codec` compresses without loss; `None` when it isn't a known audio, video or image codec.
pub fn lossless(codec: &Codec) -> Option<bool> {
    match codec {
        Codec::Flac
        | Codec::Pcm { .. }
        | Codec::Alac
        | Codec::WmaLossless
        | Codec::TrueHd
        | Codec::WavPack
        | Codec::Ape
        | Codec::Png
        | Codec::Gif
        | Codec::Bmp
        | Codec::Tiff => Some(true),
        Codec::Other(_) => None,
        _ => Some(false),
    }
}

/// The MIME type of an embedded picture's codec.
pub fn picture_mime(codec: &str) -> String {
    match codec {
        "mjpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "tiff" => "image/tiff",
        _ => "application/octet-stream",
    }
    .to_owned()
}

/// The neutral key of an FFmpeg metadata key, or `None` for keys that aren't tags.
pub fn tag_key(key: &str) -> Option<TagKey> {
    let lower = key.to_ascii_lowercase();
    Some(match lower.as_str() {
        "creation_time" | "major_brand" | "minor_version" | "compatible_brands"
        | "handler_name" | "vendor_id" | "encoder_options" => return None,
        "title" => TagKey::Title,
        "artist" => TagKey::Artist,
        "album" => TagKey::Album,
        "album_artist" => TagKey::AlbumArtist,
        "date" | "year" => TagKey::Year,
        "genre" => TagKey::Genre,
        "track" => TagKey::Track,
        "disc" => TagKey::Disc,
        "comment" => TagKey::Comment,
        "composer" => TagKey::Composer,
        "copyright" => TagKey::Copyright,
        "encoder" | "encoded_by" => TagKey::Encoder,
        l if l == "lyrics" || l.starts_with("lyrics-") => TagKey::Lyrics,
        _ => TagKey::Other(key.to_owned()),
    })
}

/// The tag format a container's tags come in.
pub fn tag_system(container: &ContainerKind, id3v2: bool, id3v1: bool) -> TagSystem {
    match container {
        ContainerKind::Mp3 | ContainerKind::Aac => {
            if id3v2 {
                TagSystem::Id3v2
            } else if id3v1 {
                TagSystem::Id3v1
            } else {
                TagSystem::Unknown
            }
        }
        ContainerKind::Aiff => {
            if id3v2 {
                TagSystem::Id3v2
            } else {
                TagSystem::Unknown
            }
        }
        ContainerKind::Flac | ContainerKind::Ogg => TagSystem::Vorbis,
        ContainerKind::Mp4 | ContainerKind::QuickTime | ContainerKind::ThreeGp => TagSystem::Mp4,
        ContainerKind::Asf => TagSystem::Asf,
        ContainerKind::Matroska | ContainerKind::WebM => TagSystem::Matroska,
        ContainerKind::Wav | ContainerKind::Avi => TagSystem::Riff,
        _ => TagSystem::Unknown,
    }
}

/// Bit depth per channel and channel count of an image pixel format.
pub fn pixel_format(name: &str) -> (Option<u8>, Option<u8>) {
    match name {
        "rgb24" | "bgr24" | "pal8" | "yuv420p" | "yuvj420p" | "yuv422p" | "yuvj422p"
        | "yuv444p" | "yuvj444p" => (Some(8), Some(3)),
        "rgba" | "bgra" | "argb" | "abgr" | "yuva420p" | "yuva444p" => (Some(8), Some(4)),
        "gray" | "monob" | "monow" => (Some(8), Some(1)),
        "ya8" => (Some(8), Some(2)),
        "gray16be" | "gray16le" => (Some(16), Some(1)),
        "rgb48be" | "rgb48le" => (Some(16), Some(3)),
        "rgba64be" | "rgba64le" => (Some(16), Some(4)),
        "yuv420p10le" | "yuv420p10be" => (Some(10), Some(3)),
        "yuv420p12le" | "yuv420p12be" => (Some(12), Some(3)),
        _ => (None, None),
    }
}
