//! getID3's names for what `MediaInfo` describes neutrally, taken from getID3 1.9.25's output on the corpus
//! (`compat/media/recorded.json`).

use speedyspec_wp_media_contracts::{Codec, ContainerKind, ImageKind, TagKey, TagSystem};

/// getID3's `fileformat` (`audio` is the first audio stream's codec: MPEG audio is `mp2` or `mp3` by layer).
pub fn fileformat(container: &ContainerKind, audio: Option<&Codec>) -> Option<&'static str> {
    Some(match container {
        ContainerKind::Mp3 if audio == Some(&Codec::Mp2) => "mp2",
        ContainerKind::Mp3 => "mp3",
        ContainerKind::Mp4 | ContainerKind::QuickTime | ContainerKind::ThreeGp => "mp4",
        ContainerKind::Matroska => "matroska",
        ContainerKind::WebM => "webm",
        ContainerKind::Ogg => "ogg",
        ContainerKind::Asf => "asf",
        ContainerKind::Wav => "wav",
        ContainerKind::Avi => "avi",
        ContainerKind::Aiff => "aiff",
        ContainerKind::Flv => "flv",
        ContainerKind::MpegPs => "mpeg",
        ContainerKind::MpegTs => "ts",
        ContainerKind::Aac => "aac",
        ContainerKind::Flac => "flac",
        ContainerKind::Ac3 => "ac3",
        ContainerKind::Dts => "dts",
        ContainerKind::RealMedia => "real",
        ContainerKind::Midi => "midi",
        ContainerKind::Image(kind) => match kind {
            ImageKind::Jpeg => "jpg",
            ImageKind::Png => "png",
            ImageKind::Gif => "gif",
            ImageKind::Bmp => "bmp",
            ImageKind::Tiff => "tiff",
            ImageKind::Webp => "webp",
            _ => return None,
        },
        _ => return None,
    })
}

/// getID3's `mime_type`.
pub fn mime_type(
    container: &ContainerKind,
    has_video: bool,
    audio: Option<&Codec>,
) -> Option<&'static str> {
    Some(match container {
        ContainerKind::Mp3 => "audio/mpeg",
        ContainerKind::Mp4 | ContainerKind::QuickTime | ContainerKind::ThreeGp => {
            if has_video {
                "video/mp4"
            } else {
                "audio/mp4"
            }
        }
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
            if has_video {
                "video/ogg"
            } else if audio == Some(&Codec::Opus) {
                "audio/ogg; codecs=opus"
            } else {
                "application/ogg"
            }
        }
        ContainerKind::Asf => {
            if has_video {
                "video/x-ms-wmv"
            } else {
                "audio/x-ms-wma"
            }
        }
        ContainerKind::Wav => "audio/wav",
        ContainerKind::Avi => "video/avi",
        ContainerKind::Aiff => "audio/x-aiff",
        ContainerKind::Flv => "video/x-flv",
        ContainerKind::MpegPs => "video/mpeg",
        ContainerKind::MpegTs => "video/MP2T",
        ContainerKind::Aac => "audio/aac",
        ContainerKind::Flac => "audio/flac",
        ContainerKind::Ac3 => "audio/ac3",
        ContainerKind::Dts => "audio/dts",
        ContainerKind::RealMedia => "audio/x-realaudio",
        ContainerKind::Midi => "audio/midi",
        ContainerKind::Other(name) => match name.as_str() {
            "au" => "audio/basic",
            "voc" => "audio/voc",
            "mpegvideo" => "video/mpeg",
            _ => return None,
        },
        _ => return None,
    })
}

/// getID3's `audio.dataformat`.
pub fn audio_dataformat(container: &ContainerKind, codec: &Codec) -> Option<&'static str> {
    Some(match container {
        ContainerKind::Mp3 if codec == &Codec::Mp2 => "mp2",
        ContainerKind::Mp3 => "mp3",
        ContainerKind::Mp4 | ContainerKind::QuickTime | ContainerKind::ThreeGp => "mp4",
        ContainerKind::Flac => "flac",
        ContainerKind::Wav => "wav",
        ContainerKind::Aiff => "aiff",
        ContainerKind::Asf => "wma",
        ContainerKind::Ac3 if codec == &Codec::Eac3 => "eac3",
        ContainerKind::Ac3 => "ac3",
        ContainerKind::Dts => "dts",
        ContainerKind::Flv => "flv",
        ContainerKind::Aac => "aac",
        ContainerKind::Ogg | ContainerKind::Avi | ContainerKind::Matroska | ContainerKind::WebM => {
            match (container, codec) {
                (ContainerKind::WebM | ContainerKind::Matroska, Codec::Opus) => "A_OPUS",
                (_, Codec::Vorbis) => "vorbis",
                (_, Codec::Opus) => "opus",
                (_, Codec::Aac) => "aac",
                (_, Codec::Mp3) => "mp3",
                (_, Codec::Flac) => "flac",
                (_, Codec::Ac3) => "ac3",
                (_, Codec::Dts) => "dts",
                (_, Codec::Pcm { .. }) => "pcm",
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// getID3's `audio.codec`, for the formats where getID3 writes one.
pub fn audio_codec(
    container: &ContainerKind,
    codec: &Codec,
    bits: Option<u16>,
    channels: Option<u16>,
) -> Option<&'static str> {
    Some(match (container, codec) {
        (ContainerKind::Mp4 | ContainerKind::QuickTime | ContainerKind::ThreeGp, Codec::Aac) => {
            "ISO/IEC 14496-3 AAC"
        }
        (ContainerKind::Wav, Codec::Pcm { .. }) => {
            if bits.unwrap_or(16) > 16 || channels.unwrap_or(2) > 2 {
                "WAVE_FORMAT_EXTENSIBLE"
            } else {
                "Pulse Code Modulation (PCM)"
            }
        }
        (ContainerKind::Avi, Codec::Mp3) => "MPEG Layer-3",
        (ContainerKind::Asf, c) => wma_name(c)?,
        (ContainerKind::Flv, Codec::Mp3) => "mp3",
        _ => return None,
    })
}

/// The name ASF files carry for their WMA codec, which getID3 reports as `codec` and `encoder`.
pub fn wma_name(codec: &Codec) -> Option<&'static str> {
    Some(match codec {
        Codec::WmaV1 => "Windows Media Audio V7",
        Codec::WmaV2 => "Windows Media Audio V8",
        Codec::WmaPro => "Windows Media Audio 9 Professional",
        Codec::WmaLossless => "Windows Media Audio 9 Lossless",
        _ => return None,
    })
}

/// getID3's `audio.bitrate_mode` when the reader didn't say: what getID3 reports per format on the corpus. MP3 is
/// "vbr" because encoders write a Xing/Info frame, which getID3 reads as variable.
pub fn audio_bitrate_mode(
    container: &ContainerKind,
    codec: &Codec,
    has_video: bool,
) -> Option<&'static str> {
    if has_video && matches!(container, ContainerKind::Matroska | ContainerKind::WebM) {
        return None;
    }
    Some(match (container, codec) {
        (ContainerKind::Mp3, Codec::Mp2) => "cbr",
        (ContainerKind::Mp3, _) => "vbr",
        (ContainerKind::Avi, _) => "cbr",
        (
            _,
            Codec::Pcm { .. } | Codec::Ac3 | Codec::Eac3 | Codec::Dts | Codec::WmaV1 | Codec::WmaV2,
        ) => "cbr",
        (_, Codec::Flac | Codec::Vorbis | Codec::Opus | Codec::WmaPro | Codec::WmaLossless) => {
            "vbr"
        }
        _ => return None,
    })
}

/// getID3's `video.bitrate_mode`, for the containers where it writes one.
pub fn video_bitrate_mode(container: &ContainerKind) -> Option<&'static str> {
    match container {
        ContainerKind::Avi => Some("vbr"),
        ContainerKind::Asf => Some("cbr"),
        _ => None,
    }
}

/// Whether getID3 writes `lossless` for this stream: not for video in ISO or Matroska files, and not for the audio
/// of Matroska and WebM files with video.
pub fn has_lossless(container: &ContainerKind, video: bool, file_has_video: bool) -> bool {
    match container {
        ContainerKind::Mp4 | ContainerKind::ThreeGp => !video,
        ContainerKind::Matroska | ContainerKind::WebM => !video && !file_has_video,
        _ => true,
    }
}

/// getID3's RIFF `wformattag` for WAV and AVI audio.
pub fn wformattag(
    container: &ContainerKind,
    codec: &Codec,
    bits: Option<u16>,
    channels: Option<u16>,
) -> Option<i64> {
    match (container, codec) {
        (ContainerKind::Wav, Codec::Pcm { float: false, .. }) => {
            Some(if bits.unwrap_or(16) > 16 || channels.unwrap_or(2) > 2 {
                65534
            } else {
                1
            })
        }
        (ContainerKind::Wav | ContainerKind::Avi, Codec::Pcm { float: true, .. }) => Some(3),
        (ContainerKind::Avi, Codec::Mp3) => Some(85),
        (ContainerKind::Avi, Codec::Pcm { .. }) => Some(1),
        _ => None,
    }
}

/// getID3's `video.dataformat`.
pub fn video_dataformat(container: &ContainerKind, codec: &Codec) -> Option<&'static str> {
    Some(match container {
        ContainerKind::Mp4 | ContainerKind::QuickTime | ContainerKind::ThreeGp => {
            if *codec == Codec::Mpeg4Part2 {
                "mpeg4"
            } else {
                "quicktime"
            }
        }
        ContainerKind::Matroska | ContainerKind::WebM => match codec {
            Codec::H264 => "h264",
            Codec::Hevc => "hevc",
            Codec::Vp8 => "V_VP8",
            Codec::Vp9 => "V_VP9",
            Codec::Av1 => "V_AV1",
            Codec::Theora => "theora",
            Codec::Mpeg4Part2 => "mpeg4",
            _ => return None,
        },
        ContainerKind::Ogg => "theora",
        ContainerKind::Flv => "flv",
        ContainerKind::Avi => "avi",
        ContainerKind::Asf => "wmv",
        ContainerKind::MpegPs | ContainerKind::MpegTs => "mpeg",
        _ => return None,
    })
}

/// getID3's `video.codec`, for the formats where getID3 writes one.
pub fn video_codec(container: &ContainerKind, codec: &Codec) -> Option<&'static str> {
    Some(match (container, codec) {
        (ContainerKind::Mp4 | ContainerKind::ThreeGp, Codec::H264) => "H.264/MPEG-4 AVC",
        (ContainerKind::Flv, Codec::Other(name)) if name == "flv1" => "Sorenson H.263",
        _ => return None,
    })
}

/// getID3's `audio.channelmode`.
pub fn channelmode(container: &ContainerKind, channels: u16) -> &'static str {
    match (container, channels) {
        (ContainerKind::Ac3, c) if c >= 2 => "surround",
        (_, 1) => "mono",
        (_, 2) => "stereo",
        _ => "surround",
    }
}

/// getID3's comment name for a tag in a given tag format.
pub fn comment_name(key: &TagKey, system: TagSystem) -> String {
    match key {
        TagKey::Title => "title".into(),
        TagKey::Artist => "artist".into(),
        TagKey::Album => "album".into(),
        TagKey::AlbumArtist => "band".into(),
        TagKey::Year => "year".into(),
        TagKey::Genre => "genre".into(),
        TagKey::Track => "track_number".into(),
        TagKey::Disc => "part_of_a_set".into(),
        TagKey::Comment => {
            if system == TagSystem::Id3v2 {
                "text".into()
            } else {
                "comment".into()
            }
        }
        TagKey::Composer => "composer".into(),
        TagKey::Copyright => "copyright_message".into(),
        TagKey::Encoder => {
            if system == TagSystem::Id3v2 {
                "encoder_settings".into()
            } else {
                "encoder".into()
            }
        }
        TagKey::Lyrics => "unsynchronised_lyric".into(),
        TagKey::Other(name) => name.to_ascii_lowercase(),
        _ => "unknown".into(),
    }
}

/// The key getID3 files a tag format's own comments under (`quicktime`, `ogg`…), besides the merged `comments`.
pub fn system_key(system: TagSystem) -> Option<&'static str> {
    Some(match system {
        TagSystem::Id3v2 => "id3v2",
        TagSystem::Id3v1 => "id3v1",
        TagSystem::Vorbis => "ogg",
        TagSystem::Mp4 => "quicktime",
        TagSystem::Asf => "asf",
        TagSystem::Matroska => "matroska",
        TagSystem::Riff => "riff",
        TagSystem::Ape => "ape",
        _ => return None,
    })
}
