//! The Data Mapper from FFmpeg's description (`RawProbe`) to `MediaInfo`, keeping FFmpeg's own view as a raw tree
//! shaped like `ffprobe -show_format -show_streams`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use speedyspec_wp_media_contracts::{
    AudioStream, Codec, ContainerKind, ImageStream, MediaInfo, Picture, RawViews, Stream, Tag,
    Tree, VideoStream,
};

use crate::names;
use crate::{RawProbe, RawStream};

const VIDEO: i32 = 0;
const AUDIO: i32 = 1;

/// `probe` as a library-neutral `MediaInfo`.
pub fn map(probe: RawProbe) -> MediaInfo {
    let brand = probe
        .tags
        .iter()
        .find(|(k, _)| k == "major_brand")
        .map(|(_, v)| v.as_str());
    let codecs: Vec<Codec> = probe
        .streams
        .iter()
        .filter(|s| !s.attached_pic)
        .map(|s| names::codec(&s.codec_name))
        .collect();
    let container = names::container(&probe.format_name, brand, &codecs);
    let is_image = matches!(container, ContainerKind::Image(_));
    let system = names::tag_system(&container, probe.id3v2, probe.id3v1);

    let mut streams = Vec::new();
    let mut pictures = Vec::new();
    for s in &probe.streams {
        let codec = names::codec(&s.codec_name);
        if s.attached_pic {
            if let Some(data) = &s.picture {
                pictures.push(Picture {
                    data: data.clone(),
                    mime: names::picture_mime(&s.codec_name),
                    width: nonzero(s.width),
                    height: nonzero(s.height),
                    kind: s
                        .tags
                        .iter()
                        .find(|(k, _)| k == "comment")
                        .map(|(_, v)| v.clone()),
                    system,
                });
            }
            continue;
        }
        streams.push(match s.media_type {
            AUDIO => Stream::Audio(AudioStream {
                lossless: names::lossless(&codec),
                bits_per_sample: bits(s),
                codec,
                sample_rate: nonzero(s.sample_rate),
                channels: (s.channels > 0).then_some(s.channels),
                channel_layout: s.channel_layout.clone(),
                bitrate: (s.bit_rate > 0).then_some(s.bit_rate),
                bitrate_mode: None,
                encoder: None,
            }),
            VIDEO if is_image => {
                let ContainerKind::Image(kind) = &container else {
                    unreachable!()
                };
                let (bit_depth, channels) = s
                    .pix_fmt
                    .as_deref()
                    .map(names::pixel_format)
                    .unwrap_or((None, None));
                Stream::Image(ImageStream {
                    kind: kind.clone(),
                    width: nonzero(s.width),
                    height: nonzero(s.height),
                    bit_depth,
                    channels,
                    animated: probe.format_name == "webp_anim"
                        || probe.format_name == "jpegxl_anim"
                        || probe.format_name == "apng",
                    lossless: names::lossless(&codec),
                })
            }
            VIDEO => Stream::Video(VideoStream {
                lossless: names::lossless(&codec),
                codec,
                width: nonzero(s.width),
                height: nonzero(s.height),
                frame_rate: s.frame_rate,
                bitrate: (s.bit_rate > 0).then_some(s.bit_rate),
                encoder: None,
            }),
            _ => Stream::Data { codec },
        });
    }

    let has_video = streams.iter().any(|s| matches!(s, Stream::Video(_)));
    let has_theora = streams
        .iter()
        .any(|s| matches!(s, Stream::Video(v) if v.codec == Codec::Theora));
    // Ogg keeps its Vorbis comments (and the vendor string, as `encoder`) on the streams, not the file.
    let mut raw_tags = probe.tags.clone();
    if container == ContainerKind::Ogg
        && let Some(first) = probe
            .streams
            .iter()
            .find(|s| !s.attached_pic && (s.media_type == AUDIO || s.media_type == VIDEO))
    {
        raw_tags.extend(first.tags.iter().filter(|(k, _)| k != "language").cloned());
    }
    let tags = tags(&raw_tags, system);
    let created_at = probe
        .tags
        .iter()
        .find(|(k, _)| k == "creation_time")
        .and_then(|(_, v)| parse_time(v));
    let tree = raw_tree(&probe);

    MediaInfo {
        mime_type: names::mime(&container, has_video, has_theora),
        container,
        duration: probe.duration_us.map(|us| Duration::from_micros(us as u64)),
        bitrate: probe.bit_rate.map(|b| b as u64),
        file_size: Some(probe.file_size),
        created_at,
        streams,
        tags,
        pictures,
        raw: RawViews {
            getid3: None,
            reader: Some(("ffmpeg".to_owned(), tree)),
        },
    }
}

fn nonzero(v: u32) -> Option<u32> {
    (v > 0).then_some(v)
}

fn bits(s: &RawStream) -> Option<u16> {
    [s.bits_per_raw_sample, s.bits_per_coded_sample]
        .into_iter()
        .find(|b| *b > 0)
}

/// Tags in file order, one per key, each keeping every value FFmpeg gave (a `;`-joined value stays one value).
fn tags(raw: &[(String, String)], system: speedyspec_wp_media_contracts::TagSystem) -> Vec<Tag> {
    let mut out: Vec<Tag> = Vec::new();
    for (key, value) in raw {
        let Some(key) = names::tag_key(key) else {
            continue;
        };
        match out.iter_mut().find(|t| t.key == key) {
            Some(tag) => tag.values.push(value.clone()),
            None => out.push(Tag {
                key,
                system,
                values: vec![value.clone()],
            }),
        }
    }
    out
}

/// An RFC 3339 time (`2024-05-06T07:08:09.5Z`, `…+02:00`, or a space for `T`). The epoch itself and anything
/// unparsable are "unknown", since muxers write zero when they have no date.
pub(crate) fn parse_time(text: &str) -> Option<SystemTime> {
    let text = text.trim();
    let b = text.as_bytes();
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || !(b[10] == b'T' || b[10] == b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut rest = &text[19..];
    let mut nanos = 0u32;
    if let Some(frac) = rest.strip_prefix('.') {
        let digits: String = frac.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            return None;
        }
        let padded: String = digits
            .chars()
            .chain(std::iter::repeat('0'))
            .take(9)
            .collect();
        nanos = padded.parse().ok()?;
        rest = &frac[digits.len()..];
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        tz if (tz.starts_with('+') || tz.starts_with('-')) && tz.len() == 6 && &tz[3..4] == ":" => {
            let sign = if tz.starts_with('-') { -1 } else { 1 };
            sign * (tz[1..3].parse::<i64>().ok()? * 3600 + tz[4..6].parse::<i64>().ok()? * 60)
        }
        _ => return None,
    };
    let secs =
        days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset;
    if secs <= 0 {
        return None;
    }
    Some(UNIX_EPOCH + Duration::new(secs as u64, nanos))
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn tag_tree(tags: &[(String, String)]) -> Tree {
    Tree::Map(
        tags.iter()
            .map(|(k, v)| (k.clone(), Tree::Str(v.clone())))
            .collect(),
    )
}

fn raw_tree(probe: &RawProbe) -> Tree {
    let mut format = vec![(
        "format_name".to_owned(),
        Tree::Str(probe.format_name.clone()),
    )];
    if let Some(us) = probe.duration_us {
        format.push(("duration".to_owned(), Tree::Float(us as f64 / 1_000_000.0)));
    }
    if let Some(rate) = probe.bit_rate {
        format.push(("bit_rate".to_owned(), Tree::Int(rate)));
    }
    format.push(("size".to_owned(), Tree::Int(probe.file_size as i64)));
    format.push(("tags".to_owned(), tag_tree(&probe.tags)));

    let streams = probe
        .streams
        .iter()
        .enumerate()
        .map(|(index, s)| {
            let codec_type = match s.media_type {
                VIDEO => "video",
                AUDIO => "audio",
                2 => "data",
                3 => "subtitle",
                4 => "attachment",
                _ => "unknown",
            };
            let mut m = vec![
                ("index".to_owned(), Tree::Int(index as i64)),
                ("codec_type".to_owned(), Tree::Str(codec_type.to_owned())),
                ("codec_name".to_owned(), Tree::Str(s.codec_name.clone())),
            ];
            let mut opt_str = |key: &str, v: &Option<String>| {
                if let Some(v) = v {
                    m.push((key.to_owned(), Tree::Str(v.clone())));
                }
            };
            opt_str("profile", &s.profile);
            opt_str("channel_layout", &s.channel_layout);
            opt_str("pix_fmt", &s.pix_fmt);
            opt_str("sample_fmt", &s.sample_fmt);
            for (key, v) in [
                ("width", u64::from(s.width)),
                ("height", u64::from(s.height)),
                ("sample_rate", u64::from(s.sample_rate)),
                ("channels", u64::from(s.channels)),
                ("bits_per_raw_sample", u64::from(s.bits_per_raw_sample)),
                ("bit_rate", s.bit_rate),
            ] {
                if v > 0 {
                    m.push((key.to_owned(), Tree::Int(v as i64)));
                }
            }
            if let Some((num, den)) = s.frame_rate {
                m.push(("r_frame_rate".to_owned(), Tree::Str(format!("{num}/{den}"))));
            }
            m.push((
                "disposition".to_owned(),
                Tree::Map(vec![(
                    "attached_pic".to_owned(),
                    Tree::Int(i64::from(s.attached_pic)),
                )]),
            ));
            m.push(("tags".to_owned(), tag_tree(&s.tags)));
            Tree::Map(m)
        })
        .collect();

    Tree::Map(vec![
        ("format".to_owned(), Tree::Map(format)),
        ("streams".to_owned(), Tree::List(streams)),
    ])
}
