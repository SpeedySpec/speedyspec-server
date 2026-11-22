//! The getID3-shaped view of a `MediaInfo` (Data Mapper): the keys WordPress reads from getID3's result, and the ones
//! plugins commonly read, in getID3's names and places. getID3 itself never runs.

mod tables;

use speedyspec_wp_media_contracts::{
    AudioStream, Codec, ContainerKind, MediaInfo, Stream, TagSystem, Tree, VideoStream,
};

/// The seconds of an ISO file's zero `mvhd` creation time (1904-01-01), which getID3 reports for files with none.
const QUICKTIME_ZERO: i64 = -2_082_844_800;

/// An ordered map under construction.
#[derive(Default)]
struct Map(Vec<(String, Tree)>);

impl Map {
    fn set(&mut self, key: &str, value: Tree) -> &mut Self {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key.to_owned(), value)),
        }
        self
    }
    fn opt(&mut self, key: &str, value: Option<Tree>) -> &mut Self {
        if let Some(value) = value {
            self.set(key, value);
        }
        self
    }
    fn entry(&mut self, key: &str) -> &mut Vec<(String, Tree)> {
        if !self.0.iter().any(|(k, _)| k == key) {
            self.0.push((key.to_owned(), Tree::Map(Vec::new())));
        }
        match self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v) {
            Some(Tree::Map(m)) => m,
            _ => unreachable!("entry holds a map"),
        }
    }
    fn tree(self) -> Tree {
        Tree::Map(self.0)
    }
}

fn s(text: &str) -> Tree {
    Tree::Bytes(text.as_bytes().to_vec())
}

fn int(v: impl Into<i64>) -> Tree {
    Tree::Int(v.into())
}

/// getID3's `PlaytimeString()`: the seconds rounded, as `m:ss`, or `h:mm:ss` from an hour up.
pub fn playtime_string(seconds: f64) -> String {
    let sign = if seconds < 0.0 { "-" } else { "" };
    let total = seconds.abs().round() as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{sign}{h}:{m:02}:{s:02}")
    } else {
        format!("{sign}{m}:{s:02}")
    }
}

fn audio_map(container: &ContainerKind, a: &AudioStream) -> Map {
    let mut m = Map::default();
    m.opt(
        "dataformat",
        tables::audio_dataformat(container, &a.codec).map(s),
    );
    m.opt(
        "codec",
        tables::audio_codec(container, &a.codec, a.bits_per_sample, a.channels).map(s),
    );
    m.opt(
        "wformattag",
        tables::wformattag(container, &a.codec, a.bits_per_sample, a.channels).map(int),
    );
    m.opt("sample_rate", a.sample_rate.map(int));
    m.opt("channels", a.channels.map(int));
    m.opt("bits_per_sample", a.bits_per_sample.map(int));
    m.opt("bitrate", a.bitrate.map(|b| int(b as i64)));
    m.opt(
        "bitrate_mode",
        a.bitrate_mode.map(|mode| {
            s(
                if mode == speedyspec_wp_media_contracts::BitrateMode::Variable {
                    "vbr"
                } else {
                    "cbr"
                },
            )
        }),
    );
    m.opt("lossless", a.lossless.map(Tree::Bool));
    if let (ContainerKind::Asf, Some(name)) = (container, tables::wma_name(&a.codec)) {
        m.set("encoder", s(name));
    } else {
        m.opt("encoder", a.encoder.as_deref().map(s));
    }
    m.opt(
        "channelmode",
        a.channels.map(|c| s(tables::channelmode(container, c))),
    );
    m
}

fn video_map(container: &ContainerKind, v: &VideoStream) -> Map {
    let mut m = Map::default();
    m.opt(
        "dataformat",
        tables::video_dataformat(container, &v.codec).map(s),
    );
    m.opt("resolution_x", v.width.map(int));
    m.opt("resolution_y", v.height.map(int));
    m.opt("codec", tables::video_codec(container, &v.codec).map(s));
    m.opt(
        "frame_rate",
        v.frame_rate
            .filter(|(_, d)| *d > 0)
            .map(|(n, d)| Tree::Float(f64::from(n) / f64::from(d))),
    );
    m.opt("bitrate", v.bitrate.map(|b| int(b as i64)));
    m.opt("lossless", v.lossless.map(Tree::Bool));
    m.opt("encoder", v.encoder.as_deref().map(s));
    if let (ContainerKind::Asf, Codec::Wmv(name)) = (container, &v.codec) {
        m.set("encoder", s(name));
    }
    m
}

/// The getID3-shaped view of `info`.
pub fn getid3_view(info: &MediaInfo) -> Tree {
    let mut top = Map::default();
    let audio: Vec<&AudioStream> = info
        .streams
        .iter()
        .filter_map(|st| {
            if let Stream::Audio(a) = st {
                Some(a)
            } else {
                None
            }
        })
        .collect();
    let video: Vec<&VideoStream> = info
        .streams
        .iter()
        .filter_map(|st| {
            if let Stream::Video(v) = st {
                Some(v)
            } else {
                None
            }
        })
        .collect();
    let c = &info.container;

    top.opt("filesize", info.file_size.map(|n| int(n as i64)));
    top.opt("fileformat", tables::fileformat(c).map(s));
    if let Some(first) = audio.first() {
        let mut a = audio_map(c, first);
        let streams = audio.iter().map(|st| audio_map(c, st).tree()).collect();
        a.set("streams", Tree::List(streams));
        top.set("audio", a.tree());
    }
    if let Some(first) = video.first() {
        top.set("video", video_map(c, first).tree());
    }
    top.opt(
        "mime_type",
        tables::mime_type(c, !video.is_empty(), audio.first().map(|a| &a.codec)).map(s),
    );

    // Tags: each format's own comments, the merged `comments`, and pictures.
    let mut merged = Map::default();
    let mut per_system: Vec<(&'static str, Map)> = Vec::new();
    for tag in &info.tags {
        let name = tables::comment_name(&tag.key, tag.system);
        let values = Tree::List(tag.values.iter().map(|v| s(v)).collect());
        if let Some(key) = tables::system_key(tag.system) {
            match per_system.iter_mut().find(|(k, _)| *k == key) {
                Some((_, m)) => {
                    m.set(&name, values.clone());
                }
                None => {
                    let mut m = Map::default();
                    m.set(&name, values.clone());
                    per_system.push((key, m));
                }
            }
        }
        if !merged.0.iter().any(|(k, _)| *k == name) {
            merged.set(&name, values);
        }
    }
    // An ID3v1 tag always has its five text fields, empty or not.
    if let Some((_, v1)) = per_system.iter_mut().find(|(k, _)| *k == "id3v1") {
        for field in ["title", "artist", "album", "year", "comment"] {
            if !v1.0.iter().any(|(k, _)| k == field) {
                v1.set(field, Tree::List(vec![s("")]));
            }
        }
    }
    let mut apic = Vec::new();
    let mut pictures = Vec::new();
    for p in &info.pictures {
        let mut m = Map::default();
        m.set("data", Tree::Bytes(p.data.clone()));
        m.set("image_mime", s(&p.mime));
        m.opt("image_width", p.width.map(int));
        m.opt("image_height", p.height.map(int));
        pictures.push(m.tree());
        if p.system == TagSystem::Id3v2 {
            let mut m = Map::default();
            m.set("data", Tree::Bytes(p.data.clone()));
            m.set("image_mime", s(&p.mime));
            m.set(
                "picturetype",
                s(p.kind.as_deref().unwrap_or("Cover (front)")),
            );
            m.opt("image_width", p.width.map(int));
            m.opt("image_height", p.height.map(int));
            apic.push(m.tree());
        }
    }
    if !pictures.is_empty() {
        merged.set("picture", Tree::List(pictures));
    }
    for (key, comments) in per_system {
        top.entry(key).push(("comments".into(), comments.tree()));
    }
    if !apic.is_empty() {
        top.entry("id3v2").push(("APIC".into(), Tree::List(apic)));
    }
    if !merged.0.is_empty() {
        top.set("comments", merged.tree());
    }

    // Creation time, where wp_get_media_creation_timestamp() looks for it.
    let created = info
        .created_at
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    match c {
        ContainerKind::Mp4 | ContainerKind::QuickTime | ContainerKind::ThreeGp => {
            let atom = Tree::Map(vec![(
                "creation_time_unix".into(),
                int(created.unwrap_or(QUICKTIME_ZERO)),
            )]);
            top.entry("quicktime").push((
                "moov".into(),
                Tree::Map(vec![("subatoms".into(), Tree::List(vec![atom]))]),
            ));
        }
        ContainerKind::Asf => {
            let props = Tree::Map(vec![(
                "creation_date_unix".into(),
                int(created.unwrap_or(0)),
            )]);
            top.entry("asf")
                .push(("file_properties_object".into(), props));
        }
        ContainerKind::Matroska | ContainerKind::WebM => {
            if let Some(secs) = created {
                let info0 = Tree::Map(vec![("DateUTC_unix".into(), int(secs))]);
                top.entry("matroska")
                    .push(("info".into(), Tree::List(vec![info0])));
            }
        }
        _ => {}
    }

    if let Some(d) = info.duration {
        let seconds = d.as_secs_f64();
        top.set("playtime_seconds", Tree::Float(seconds));
        top.set("playtime_string", s(&playtime_string(seconds)));
    }
    top.opt("bitrate", info.bitrate.map(|b| int(b as i64)));
    top.tree()
}

/// `info` with its getID3 view filled in.
pub fn with_getid3(mut info: MediaInfo) -> MediaInfo {
    info.raw.getid3 = Some(getid3_view(&info));
    info
}
