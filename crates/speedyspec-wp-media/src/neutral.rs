//! The library-neutral description of a file as a PHP array: `$data['speedyspec']`, and what a PHP replacement
//! reader returns (`schema` 1).

use std::time::UNIX_EPOCH;

use speedyspec_wp_media_contracts::{
    BitrateMode, Codec, ContainerKind, ImageKind, MediaInfo, Stream, TagKey, TagSystem, Tree,
};

pub const SCHEMA: i64 = 1;

fn b(s: &str) -> Tree {
    Tree::Bytes(s.as_bytes().to_vec())
}

fn opt<T>(m: &mut Vec<(String, Tree)>, key: &str, value: Option<T>, f: impl Fn(T) -> Tree) {
    if let Some(v) = value {
        m.push((key.to_owned(), f(v)));
    }
}

pub(crate) fn container_name(c: &ContainerKind) -> String {
    match c {
        ContainerKind::Image(kind) => format!("image/{}", image_name(kind)),
        ContainerKind::Other(name) => format!("other/{name}"),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

pub(crate) fn image_name(kind: &ImageKind) -> String {
    match kind {
        ImageKind::Other(name) => name.clone(),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

pub(crate) fn codec_name(c: &Codec) -> String {
    match c {
        Codec::Pcm { bits, float } => format!("pcm_{}{bits}", if *float { "f" } else { "s" }),
        Codec::RealAudio(n) | Codec::Wmv(n) | Codec::RealVideo(n) | Codec::Other(n) => n.clone(),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

pub(crate) fn tag_key_name(k: &TagKey) -> String {
    match k {
        TagKey::Other(n) => n.clone(),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

pub(crate) fn system_name(s: TagSystem) -> String {
    format!("{s:?}").to_ascii_lowercase()
}

/// `info` as the neutral array, with the reader's raw view under its name (`ffmpeg`, `midi`).
pub fn neutral(info: &MediaInfo) -> Tree {
    let mut m: Vec<(String, Tree)> = vec![
        ("schema".into(), Tree::Int(SCHEMA)),
        ("container".into(), b(&container_name(&info.container))),
    ];
    opt(&mut m, "mime_type", info.mime_type.as_deref(), b);
    opt(&mut m, "duration", info.duration, |d| {
        Tree::Float(d.as_secs_f64())
    });
    opt(&mut m, "bitrate", info.bitrate, |v| Tree::Int(v as i64));
    opt(&mut m, "file_size", info.file_size, |v| Tree::Int(v as i64));
    opt(
        &mut m,
        "created_at",
        info.created_at
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok()),
        |d| Tree::Int(d.as_secs() as i64),
    );
    let streams = info
        .streams
        .iter()
        .map(|s| {
            let mut e: Vec<(String, Tree)> = Vec::new();
            match s {
                Stream::Audio(a) => {
                    e.push(("type".into(), b("audio")));
                    e.push(("codec".into(), b(&codec_name(&a.codec))));
                    opt(&mut e, "sample_rate", a.sample_rate, |v| {
                        Tree::Int(v.into())
                    });
                    opt(&mut e, "channels", a.channels, |v| Tree::Int(v.into()));
                    opt(&mut e, "channel_layout", a.channel_layout.as_deref(), b);
                    opt(&mut e, "bits_per_sample", a.bits_per_sample, |v| {
                        Tree::Int(v.into())
                    });
                    opt(&mut e, "lossless", a.lossless, Tree::Bool);
                    opt(&mut e, "bitrate", a.bitrate, |v| Tree::Int(v as i64));
                    opt(&mut e, "bitrate_mode", a.bitrate_mode, |m| {
                        b(if m == BitrateMode::Variable {
                            "vbr"
                        } else {
                            "cbr"
                        })
                    });
                    opt(&mut e, "encoder", a.encoder.as_deref(), b);
                }
                Stream::Video(v) => {
                    e.push(("type".into(), b("video")));
                    e.push(("codec".into(), b(&codec_name(&v.codec))));
                    opt(&mut e, "width", v.width, |x| Tree::Int(x.into()));
                    opt(&mut e, "height", v.height, |x| Tree::Int(x.into()));
                    opt(&mut e, "frame_rate", v.frame_rate, |(n, d)| {
                        Tree::List(vec![Tree::Int(n.into()), Tree::Int(d.into())])
                    });
                    opt(&mut e, "bitrate", v.bitrate, |x| Tree::Int(x as i64));
                    opt(&mut e, "lossless", v.lossless, Tree::Bool);
                    opt(&mut e, "encoder", v.encoder.as_deref(), b);
                }
                Stream::Image(i) => {
                    e.push(("type".into(), b("image")));
                    e.push(("kind".into(), b(&image_name(&i.kind))));
                    opt(&mut e, "width", i.width, |x| Tree::Int(x.into()));
                    opt(&mut e, "height", i.height, |x| Tree::Int(x.into()));
                    opt(&mut e, "bit_depth", i.bit_depth, |x| Tree::Int(x.into()));
                    opt(&mut e, "channels", i.channels, |x| Tree::Int(x.into()));
                    e.push(("animated".into(), Tree::Bool(i.animated)));
                    opt(&mut e, "lossless", i.lossless, Tree::Bool);
                }
                Stream::Data { codec } => {
                    e.push(("type".into(), b("data")));
                    e.push(("codec".into(), b(&codec_name(codec))));
                }
                _ => e.push(("type".into(), b("unknown"))),
            }
            Tree::Map(e)
        })
        .collect();
    m.push(("streams".into(), Tree::List(streams)));
    let tags = info
        .tags
        .iter()
        .map(|t| {
            Tree::Map(vec![
                ("key".into(), b(&tag_key_name(&t.key))),
                ("system".into(), b(&system_name(t.system))),
                (
                    "values".into(),
                    Tree::List(t.values.iter().map(|v| b(v)).collect()),
                ),
            ])
        })
        .collect();
    m.push(("tags".into(), Tree::List(tags)));
    let pictures = info
        .pictures
        .iter()
        .map(|p| {
            let mut e = vec![("mime".to_owned(), b(&p.mime))];
            opt(&mut e, "width", p.width, |x| Tree::Int(x.into()));
            opt(&mut e, "height", p.height, |x| Tree::Int(x.into()));
            opt(&mut e, "kind", p.kind.as_deref(), b);
            e.push(("system".into(), b(&system_name(p.system))));
            e.push(("data".into(), Tree::Bytes(p.data.clone())));
            Tree::Map(e)
        })
        .collect();
    m.push(("pictures".into(), Tree::List(pictures)));
    if let Some((name, tree)) = &info.raw.reader {
        m.push((name.clone(), tree.clone()));
    }
    Tree::Map(m)
}
